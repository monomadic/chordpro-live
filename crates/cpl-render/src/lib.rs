//! CoreText coverage composed into a page texture, presented with wgpu.
//! This first renderer rebuilds the page only when its content or size changes.

mod text;

pub use text::CoreText;

use cpl_app::{Face, Frame, Ink, Polarity};
use std::{collections::HashMap, sync::Arc};

#[derive(Hash, PartialEq, Eq)]
struct RasterKey {
    content: String,
    size: u32,
    scale: u32,
    weight: u16,
    face: Face,
}

#[derive(Default)]
struct RasterCache {
    entries: HashMap<RasterKey, Arc<text::Raster>>,
    bytes: usize,
}

impl RasterCache {
    fn get(&mut self, item: &cpl_app::Text, scale: f32) -> Option<Arc<text::Raster>> {
        let key = RasterKey {
            content: item.content.clone(),
            size: item.size.to_bits(),
            scale: scale.to_bits(),
            weight: item.weight,
            face: item.face,
        };
        if let Some(raster) = self.entries.get(&key) {
            return Some(Arc::clone(raster));
        }
        let raster = Arc::new(CoreText.rasterize(
            &item.content,
            item.size,
            scale,
            item.weight,
            item.face,
        )?);
        const BUDGET: usize = 32 * 1024 * 1024;
        let bytes = raster.coverage.len();
        if self.bytes + bytes > BUDGET {
            self.entries.clear();
            self.bytes = 0;
        }
        if bytes <= BUDGET {
            self.bytes += bytes;
            self.entries.insert(key, Arc::clone(&raster));
        }
        Some(raster)
    }
}

pub struct PageImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Default)]
pub struct PageCompositor {
    rasters: RasterCache,
}

impl PageCompositor {
    pub fn compose(&mut self, frame: &Frame, scale: f32, width: u32, height: u32) -> PageImage {
        let width_usize = width as usize;
        let height_usize = height as usize;
        let paper: u8 = if frame.polarity == Polarity::Light {
            255
        } else {
            0
        };
        let mut pixels = vec![paper; width_usize * height_usize * 4];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        for item in &frame.text {
            let Some(raster) = self.rasters.get(item, scale) else {
                continue;
            };
            let ink = match (frame.polarity, item.ink) {
                (Polarity::Light, Ink::Strong) => 0,
                (Polarity::Light, Ink::Soft) => 74,
                (Polarity::Light, Ink::Faint) => 145,
                (Polarity::Dark, Ink::Strong) => 255,
                (Polarity::Dark, Ink::Soft) => 181,
                (Polarity::Dark, Ink::Faint) => 110,
            };
            let left = (item.x * scale).round() as i32 - text::RASTER_PAD;
            let top = (item.baseline * scale).round() as i32 - raster.baseline as i32;
            for row in 0..raster.height as i32 {
                let y = top + row;
                if y < 0 || y >= height_usize as i32 {
                    continue;
                }
                for col in 0..raster.width as i32 {
                    let x = left + col;
                    if x < 0 || x >= width_usize as i32 {
                        continue;
                    }
                    let alpha =
                        raster.coverage[row as usize * raster.width as usize + col as usize] as u16;
                    if alpha == 0 {
                        continue;
                    }
                    let offset = (y as usize * width_usize + x as usize) * 4;
                    for channel in &mut pixels[offset..offset + 3] {
                        *channel =
                            (((*channel as u16) * (255 - alpha) + ink as u16 * alpha) / 255) as u8;
                    }
                }
            }
        }
        PageImage {
            width,
            height,
            pixels,
        }
    }
}

pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    image_size: (u32, u32),
}

impl Gpu {
    pub async fn new(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .unwrap_or(capabilities.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: capabilities.alpha_modes[0],
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("page texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("page sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("page shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("page.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("page pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("page pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let texture = make_texture(&device, config.width, config.height);
        let bind_group = make_bind_group(&device, &layout, &texture, &sampler);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            layout,
            sampler,
            texture,
            bind_group,
            image_size: (width.max(1), height.max(1)),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn upload_image(&mut self, image: &PageImage) {
        if (image.width, image.height) != (self.config.width, self.config.height) {
            return;
        }
        if self.image_size != (image.width, image.height) {
            self.texture = make_texture(&self.device, image.width, image.height);
            self.bind_group =
                make_bind_group(&self.device, &self.layout, &self.texture, &self.sampler);
            self.image_size = (image.width, image.height);
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &image.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn render(&self) -> bool {
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                return false;
            }
            _ => return true,
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("page encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("page pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(output);
        true
    }
}

fn make_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("song page"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn make_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("song page bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use cpl_app::{Ink, Text};

    #[test]
    fn same_text_at_same_scale_reuses_coverage() {
        let item = Text {
            content: "A lyric line".into(),
            group: 0,
            x: 10.0,
            baseline: 20.0,
            size: 17.0,
            weight: 400,
            face: Face::System,
            ink: Ink::Strong,
        };
        let mut cache = RasterCache::default();
        let first = cache.get(&item, 2.0).unwrap();
        let second = cache.get(&item, 2.0).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(cache.entries.len(), 1);
        cache.get(&item, 1.0).unwrap();
        assert_eq!(cache.entries.len(), 2);
    }

    #[test]
    fn a_composed_page_has_complete_pixels_in_both_polarities() {
        let mut compositor = PageCompositor::default();
        let mut frame = Frame {
            width: 120.0,
            height: 60.0,
            polarity: Polarity::Light,
            text: vec![Text {
                content: "Page".into(),
                group: 0,
                x: 10.0,
                baseline: 30.0,
                size: 17.0,
                weight: 400,
                face: Face::System,
                ink: Ink::Strong,
            }],
            fits: true,
            columns: 1,
            ladder_step: 0,
        };
        let light = compositor.compose(&frame, 2.0, 240, 120);
        frame.polarity = Polarity::Dark;
        let dark = compositor.compose(&frame, 2.0, 240, 120);
        assert_eq!(light.pixels.len(), 240 * 120 * 4);
        assert!(light.pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
        assert_eq!(&light.pixels[..4], &[255, 255, 255, 255]);
        assert_eq!(&dark.pixels[..4], &[0, 0, 0, 255]);
        assert!(light.pixels.chunks_exact(4).any(|pixel| pixel[0] < 255));
        assert!(dark.pixels.chunks_exact(4).any(|pixel| pixel[0] > 0));
    }
}
