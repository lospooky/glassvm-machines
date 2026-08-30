use wyrd16_core::{Artifact, MAX_ROM_BYTES};

#[test]
fn artifact_requires_a_nonempty_aligned_rune_stream_within_the_arena() {
    assert_eq!(Artifact::parse(&[0, 1]).expect("one rune").rune_count(), 1);
    assert!(Artifact::parse(&[]).is_err());
    assert!(Artifact::parse(&[0]).is_err());
    assert!(Artifact::parse(&vec![0; MAX_ROM_BYTES + 2]).is_err());
}
