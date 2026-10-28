//! Song measurement and the pure arithmetic that fits measured blocks into columns.

mod ladder;
mod measure;
mod tab;

pub use tab::{Staff, System, staves};

pub use ladder::{
    HeaderLayout, LADDER, Options, PageMetrics, Solution, Step, Tabs, Viewport, layout, solve,
    solve_with,
};

pub use measure::{
    FOLD_CUE, FOLD_SEPARATOR, Face, Header, Measure, MeasuredChord, MeasuredDiagram, MeasuredLine,
    MeasuredPill, MeasuredSection, MeasuredSegment, MeasuredSong, Metrics, PILLS, Role, Row, Run,
    Style, Token, measure_song,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    /// Half-open ranges into the ordered block list.
    pub columns: Vec<std::ops::Range<usize>>,
    pub body_size: f32,
    pub meets_floor: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct FitOptions {
    pub available_width: f32,
    pub available_height: f32,
    pub header_height: f32,
    pub header_gap: f32,
    pub section_gap: f32,
    pub column_gutter: f32,
    pub max_columns: usize,
    pub max_body_size: f32,
    pub floor: f32,
}

pub fn fit(blocks: &[Block], options: FitOptions) -> Fit {
    let FitOptions {
        available_width,
        available_height,
        header_height,
        header_gap,
        section_gap,
        column_gutter,
        max_columns,
        max_body_size,
        floor,
    } = options;
    let mut best = Fit {
        columns: one_column(blocks.len()),
        body_size: 0.0,
        meets_floor: false,
    };
    if available_width <= 0.0 || available_height <= 0.0 {
        return best;
    }
    if blocks.is_empty() {
        // Nothing to draw beyond the header always fits.
        return Fit {
            columns: Vec::new(),
            body_size: max_body_size,
            meets_floor: max_body_size >= floor,
        };
    }
    for count in 1..=max_columns.min(blocks.len()) {
        let columns = pack_into(blocks, count, section_gap);
        let width: f32 = columns
            .iter()
            .map(|range| {
                blocks[range.clone()]
                    .iter()
                    .map(|b| b.width)
                    .fold(0.0, f32::max)
            })
            .sum::<f32>()
            + column_gutter * (columns.len() - 1) as f32;
        let height = columns
            .iter()
            .map(|range| {
                blocks[range.clone()].iter().map(|b| b.height).sum::<f32>()
                    + section_gap * range.len().saturating_sub(1) as f32
            })
            .fold(0.0, f32::max)
            + header_height
            + header_gap;
        if width <= 0.0 || height <= 0.0 {
            continue;
        }
        let size = (available_width / width)
            .min(available_height / height)
            .min(max_body_size);
        if size > best.body_size {
            best = Fit {
                columns,
                body_size: size,
                meets_floor: size >= floor,
            };
        }
    }
    best
}

/// Greedy balanced packing, preserving every section as an indivisible block.
/// Adapted from chordpro-tui/internal/pdf/layout.go's packInto.
pub fn pack_into(blocks: &[Block], count: usize, gap: f32) -> Vec<std::ops::Range<usize>> {
    if blocks.is_empty() {
        return Vec::new();
    }
    if count <= 1 {
        return one_column(blocks.len());
    }
    let total =
        blocks.iter().map(|b| b.height).sum::<f32>() + gap * blocks.len().saturating_sub(1) as f32;
    let mut target = total / count as f32;
    loop {
        let mut result = Vec::new();
        let mut start = 0;
        let mut height = 0.0;
        for (index, block) in blocks.iter().enumerate() {
            let add = block.height + if index > start { gap } else { 0.0 };
            if index > start && height + add > target + 1e-6 {
                result.push(start..index);
                start = index;
                height = 0.0;
            }
            height += block.height + if index > start { gap } else { 0.0 };
        }
        result.push(start..blocks.len());
        if result.len() <= count {
            return result;
        }
        target = (target * 1.05).max(target + 0.001);
    }
}

fn one_column(len: usize) -> Vec<std::ops::Range<usize>> {
    std::iter::once(0..len).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_section_order_and_wholeness() {
        let blocks = [Block {
            width: 10.0,
            height: 10.0,
        }; 5];
        let columns = pack_into(&blocks, 3, 2.0);
        assert!(columns.len() <= 3);
        assert_eq!(columns.first().unwrap().start, 0);
        assert_eq!(columns.last().unwrap().end, 5);
        assert!(columns.windows(2).all(|pair| pair[0].end == pair[1].start));
    }

    #[test]
    fn chooses_more_columns_when_they_improve_readability() {
        let blocks = [Block {
            width: 10.0,
            height: 30.0,
        }; 3];
        let result = fit(
            &blocks,
            FitOptions {
                available_width: 60.0,
                available_height: 80.0,
                header_height: 0.0,
                header_gap: 0.0,
                section_gap: 2.0,
                column_gutter: 2.0,
                max_columns: 3,
                max_body_size: 20.0,
                floor: 1.5,
            },
        );
        assert_eq!(result.columns.len(), 3);
        assert!(result.meets_floor);
    }

    #[test]
    fn an_empty_song_fits() {
        let result = fit(
            &[],
            FitOptions {
                available_width: 10.0,
                available_height: 10.0,
                header_height: 0.0,
                header_gap: 0.0,
                section_gap: 0.0,
                column_gutter: 0.0,
                max_columns: 4,
                max_body_size: 1.0,
                floor: 1.0,
            },
        );
        assert!(result.meets_floor);
        assert!(result.columns.is_empty());
    }

    #[test]
    fn reports_when_floor_cannot_be_met() {
        let blocks = [Block {
            width: 100.0,
            height: 100.0,
        }];
        let result = fit(
            &blocks,
            FitOptions {
                available_width: 50.0,
                available_height: 50.0,
                header_height: 0.0,
                header_gap: 0.0,
                section_gap: 0.0,
                column_gutter: 0.0,
                max_columns: 1,
                max_body_size: 30.0,
                floor: 1.0,
            },
        );
        assert!(!result.meets_floor);
        assert_eq!(result.body_size, 0.5);
    }
}
