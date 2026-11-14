// Tests ported from CHOpt's optimiser_unittest.cpp (overlap guitar paths).
// Copyright (C) 2020-2026 Raymond Wright. Licensed under the GNU GPL v3.0 or later.

use super::engine::Engine;
use super::optimiser::Optimiser;
use super::points::PathingSettings;
use super::processed::{Path, ProcessedSong};
use super::song::*;
use super::time::{SpMode, SpTimeMap, TempoMap};
use std::sync::atomic::AtomicBool;

fn note(pos: i32) -> Note {
    make_note(pos, 0, GREEN)
}
fn sus(pos: i32, len: i32) -> Note {
    make_note(pos, len, GREEN)
}
fn gry(pos: i32) -> Note {
    make_chord(pos, &[(GREEN, 0), (RED, 0), (YELLOW, 0)])
}
fn sp(position: i32, length: i32) -> StarPower {
    StarPower { position, length }
}

fn track(notes: Vec<Note>, phrases: Vec<StarPower>) -> NoteTrack {
    let mut t = NoteTrack::new(notes, 192, false, false, 65);
    t.set_sp_phrases(phrases);
    t
}

fn optimise_with(notes: Vec<Note>, phrases: Vec<StarPower>, s: PathingSettings) -> (Path, ProcessedSong) {
    let t = track(notes, phrases);
    let tm = SpTimeMap::new(TempoMap::default(), SpMode::Measure);
    let song = ProcessedSong::new(&t, &tm, &[], &s);
    let cancel = AtomicBool::new(false);
    let path = Optimiser::new(&song, &cancel, s.whammy_delay).optimal_path().unwrap();
    (path, song)
}

fn optimise(notes: Vec<Note>, phrases: Vec<StarPower>) -> Path {
    optimise_with(notes, phrases, PathingSettings::new(Engine::clone_hero(false))).0
}

fn check_act(path: &Path, i: usize, start: usize, end: usize, sp_start: f64, sp_end: f64) {
    let a = &path.activations[i];
    assert_eq!((a.act_start, a.act_end), (start, end), "act {i} points");
    assert!((a.sp_start - sp_start).abs() < 0.01, "act {i} sp_start {} vs {sp_start}", a.sp_start);
    assert!((a.sp_end - sp_end).abs() < 0.01, "act {i} sp_end {} vs {sp_end}", a.sp_end);
}

#[test]
fn simplest_song_with_a_non_empty_path() {
    let p = optimise(vec![note(0), note(192), note(384)], vec![sp(0, 50), sp(192, 50)]);
    assert_eq!(p.score_boost, 50);
    assert_eq!(p.activations.len(), 1);
    check_act(&p, 0, 2, 2, 2.0, 18.0);
}

#[test]
fn simplest_song_with_multiple_acts() {
    let p = optimise(
        vec![note(0), note(192), gry(384), note(3840), note(4032), gry(10368)],
        vec![sp(0, 50), sp(192, 50), sp(3840, 50), sp(4032, 50)],
    );
    assert_eq!(p.score_boost, 300);
    assert_eq!(p.activations.len(), 2);
    check_act(&p, 0, 2, 2, 2.0, 18.0);
    check_act(&p, 1, 5, 5, 54.0, 70.0);
}

#[test]
fn simplest_song_with_an_act_containing_more_than_one_note() {
    let p = optimise(vec![note(0), note(192), note(384), note(576)], vec![sp(0, 50), sp(192, 50)]);
    assert_eq!(p.score_boost, 100);
    check_act(&p, 0, 2, 3, 2.0, 18.0);
}

#[test]
fn simplest_song_with_an_act_that_must_go_as_long_as_possible() {
    let p = optimise(vec![note(0), note(192), note(384), note(3360)], vec![sp(0, 50), sp(192, 50)]);
    assert_eq!(p.score_boost, 100);
    check_act(&p, 0, 2, 3, 2.0, 18.0);
}

#[test]
fn simplest_song_where_greedy_algorithm_fails() {
    let p = optimise(
        vec![note(0), note(192), note(384), make_chord(3840, &[(GREEN, 0), (RED, 0)])],
        vec![sp(0, 50), sp(192, 50)],
    );
    assert_eq!(p.score_boost, 100);
    check_act(&p, 0, 3, 3, 20.0, 36.0);
}

#[test]
fn simplest_song_where_a_phrase_must_be_hit_early() {
    let p = optimise(
        vec![note(0), note(192), note(384), note(3224), note(9378), note(15714), note(15715)],
        vec![sp(0, 50), sp(192, 50), sp(3224, 50), sp(9378, 50)],
    );
    assert_eq!(p.score_boost, 150);
    check_act(&p, 0, 2, 2, 0.8958, 16.8958);
    check_act(&p, 1, 5, 6, 81.84375, 97.84375);
}

#[test]
fn simplest_song_where_activations_ending_late_matter() {
    let mut notes = vec![note(0), note(192), note(384), sus(3234, 1440)];
    notes.extend((10944..=10953).map(note));
    let p = optimise(notes, vec![sp(0, 50), sp(192, 50), sp(3234, 50)]);
    assert_eq!(p.score_boost, 750);
    assert_eq!(p.activations.len(), 1);
}

#[test]
fn early_whammy_at_start_of_an_sp_phrase_is_always_counted() {
    let p = optimise(vec![sus(0, 1420), note(1500), note(1600)], vec![sp(0, 1550)]);
    assert_eq!(p.score_boost, 50);
    assert_eq!(p.activations.len(), 1);
}

#[test]
fn whammy_just_before_the_activation_point_is_not_double_counted() {
    let p = optimise(vec![sus(192, 1440), note(1632), note(6336)], vec![sp(192, 1), sp(1632, 1)]);
    assert!(p.score_boost < 100);
}

#[test]
fn activation_right_after_a_sp_sustain_is_drawn_correctly() {
    let p = optimise(vec![sus(0, 1488), sus(2880, 3264)], vec![sp(0, 1)]);
    assert!(p.activations[0].sp_start >= 15.0);
}

#[test]
fn songs_ending_in_es1_are_pathed_correctly() {
    let p = optimise(
        vec![note(0), note(192), note(384), note(576), note(768), note(4032), note(4224)],
        vec![sp(0, 50), sp(192, 50), sp(4032, 50)],
    );
    assert_eq!(p.score_boost, 150);
    assert_eq!(p.activations.len(), 1);
}

#[test]
fn compressed_whammy_is_specified_correctly() {
    let p = optimise(
        vec![sus(192, 192), note(672), gry(1000), note(3840), note(9984), gry(10176)],
        vec![sp(192, 50), sp(672, 50), sp(3840, 50), sp(9984, 50)],
    );
    let a = p.activations[0];
    assert_eq!(p.score_boost, 300);
    assert_eq!(p.activations.len(), 2);
    assert!(a.whammy_end > 1.06 && a.whammy_end < 1.74, "{}", a.whammy_end);
    assert!(a.sp_start < 3.6);
}

#[test]
fn acts_covering_the_last_note_do_not_compress_whammy() {
    let p = optimise(vec![sus(0, 1536), note(1728)], vec![sp(0, 50)]);
    assert!(p.activations[0].whammy_end > 16.0);
}

#[test]
fn use_next_point_to_work_out_compressed_whammy() {
    let p = optimise(
        vec![note(0), note(192), gry(384), note(3350), note(3360), note(9504), gry(9696)],
        vec![sp(0, 50), sp(192, 50), sp(3350, 50), sp(9504, 50)],
    );
    assert!(p.activations[0].whammy_end > 17.45, "{}", p.activations[0].whammy_end);
}

#[test]
fn forbidden_squeeze_does_not_grant_extra_whammy_next_act() {
    let p = optimise(
        vec![note(0), note(192), note(768), sus(3840, 192), note(4224), sus(19200, 192), note(38400), note(41990)],
        vec![sp(0, 1), sp(192, 1), sp(3840, 576), sp(19200, 1)],
    );
    assert_eq!(p.score_boost, 200);
}

#[test]
fn easier_activations_are_chosen_where_possible() {
    let p = optimise(vec![note(0), note(192), note(384), note(3504), note(9600), note(12672)], vec![sp(0, 1), sp(192, 1)]);
    assert!(p.activations[0].sp_start > 20.0);
}

#[test]
fn early_whammy_from_a_note_cannot_be_obtained_until_the_note_is_hit() {
    let p = optimise(vec![sus(0, 1392), sus(1536, 192)], vec![sp(0, 1), sp(1536, 1)]);
    assert_eq!(p.score_boost, 28);
}

#[test]
fn does_not_crash_with_positive_video_lag() {
    let mut s = PathingSettings::new(Engine::clone_hero(false));
    s.video_lag = 0.1;
    let (p, _) = optimise_with(vec![sus(192, 192)], vec![sp(192, 1)], s);
    assert_eq!(p.score_boost, 0);
}

#[test]
fn whammy_delay_is_handled_correctly() {
    let mut notes = vec![note(0), note(192), note(768), sus(3840, 1420), note(5376)];
    notes.extend((0..7).map(|i| note(13056 + 192 * i)));
    let mut s = PathingSettings::new(Engine::clone_hero(false));
    s.whammy_delay = 0.1;
    let (p, _) = optimise_with(notes, vec![sp(0, 1), sp(192, 1), sp(3840, 1728)], s);
    assert_eq!(p.activations.len(), 2);
    assert_eq!(p.score_boost, 550);
}

#[test]
fn act_end_is_constrained_by_next_act_start() {
    let mut notes = vec![sus(0, 1440), note(1536), sus(4608, 1420), note(6144)];
    notes.extend((19200..=19206).map(note));
    let p = optimise(notes, vec![sp(0, 1), sp(4608, 1537)]);
    assert!(p.activations[0].sp_end < 23.9, "{}", p.activations[0].sp_end);
}

#[test]
fn whammy_delay_to_stop_early_halfbar_is_considered() {
    let mut notes = vec![note(0), sus(192, 1536)];
    notes.extend((0..10).map(|i| note(1728 + 192 * i)));
    notes.push(note(5476));
    notes.extend((0..5).map(|i| note(19200 + 192 * i)));
    let p = optimise(notes, vec![sp(0, 1), sp(192, 1540), sp(5476, 1), sp(19200, 1)]);
    assert_eq!(p.activations.len(), 2);
    assert_eq!(p.score_boost, 1000);
}

#[test]
fn summary_matches_chopt_format() {
    let (p, song) = optimise_with(
        vec![note(0), note(192), gry(384), note(3840), note(4032), gry(10368)],
        vec![sp(0, 50), sp(192, 50), sp(3840, 50), sp(4032, 50)],
        PathingSettings::new(Engine::clone_hero(false)),
    );
    let s = song.path_summary(&p);
    assert!(s.starts_with("Path: 2-2\nNo SP score: "), "{s}");
    assert!(s.contains("Average multiplier: "), "{s}");
    assert!(s.contains("\n2: NN (GRY)"), "{s}");
}

// Truss also has tests here that run over a local chart library and save PNGs; they need Truss's
// library scanner, so they are left out of this copy.
