#!/usr/bin/perl

# A ChordPro output backend for `cpl conform`: writes what the reference
# implementation parsed, as JSON, so cpl-song's parse can be compared with
# it. It is loaded from this folder by `chordpro --generate=CplJSON` with
# this folder on PERL5LIB; nothing is installed into the reference.
#
# Each song is its metadata and its body: one element per line the
# reference produced, in order, with the element's type and context and
# the fields `cpl conform` compares. Chords are given as the reference
# displays them, after any {transpose}.
#
# {"songs": [{"meta": {"title": ["…"], …},
#             "body": [{"type": "songline", "context": "verse",
#                       "chords": ["", "Am", …], "phrases": ["…", …]}, …]}]}

package ChordPro::Output::CplJSON;

use strict;
use warnings;
use JSON::PP;

sub chord_text {
    my ($chord) = @_;
    return "" unless ref $chord;
    my $shown = eval { $chord->chord_display };
    return defined $shown ? "$shown" : ( $chord->key // "" );
}

sub element {
    my ($item) = @_;
    my %out = (
        type    => $item->{type},
        context => $item->{context} // "",
    );
    if ( $item->{type} eq "songline" ) {
        $out{chords}  = [ map { chord_text($_) } @{ $item->{chords} // [] } ];
        $out{phrases} = [ map { "$_" } @{ $item->{phrases} // [] } ];
    }
    elsif ( $item->{type} eq "rechorus" ) {
        $out{chorus} = [ map { element($_) } @{ $item->{chorus} // [] } ];
    }
    elsif ( $item->{type} eq "set" ) {
        $out{name}  = $item->{name};
        $out{value} = ref $item->{value} ? "" : ( $item->{value} // "" );
    }
    # Every other plain field, as written: text, labels, comment origins.
    while ( my ( $field, $value ) = each %$item ) {
        next if exists $out{$field} || $field eq "line" || !defined $value || ref $value;
        $out{$field} = "$value";
    }
    \%out;
}

sub generate_songbook {
    my ( $self, $sb ) = @_;
    my @songs;
    for my $song ( @{ $sb->{songs} } ) {
        my %meta;
        while ( my ( $name, $values ) = each %{ $song->{meta} // {} } ) {
            next unless ref $values eq "ARRAY";
            $meta{$name} = [ map { ref $_ ? () : "$_" } @$values ];
        }
        push @songs, {
            meta => \%meta,
            body => [ map { element($_) } @{ $song->{body} // [] } ],
        };
    }
    [ JSON::PP->new->canonical->utf8(0)->pretty->encode( { songs => \@songs } ) ];
}

1;
