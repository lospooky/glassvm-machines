use serde::{Deserialize, Serialize};

pub(super) const SESSION_SNAPSHOT_PAYLOAD_DOMAIN: &str =
    "glassvm.chip8.session-snapshot.payload.v5";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SessionLifecycle {
    Fresh,
    Incremental,
    Terminal,
}

pub const SNAP_MEM: usize = 65536;
pub const SNAP_BUF: usize = 8192; // 128 × 64
pub const SNAP_MAGIC: [u8; 8] = *b"GLVMCSNP";
pub const SNAP_CODEC_VERSION: u16 = 1;
pub const SNAP_HEADER_LEN: usize = SNAP_MAGIC.len() + 2 + 4;
pub const SNAP_PAYLOAD_LEN: usize =
    SNAP_MEM + 16 + 2 + 2 + 32 + 1 + 1 + 1 + SNAP_BUF * 2 + 1 + 1 + 2 + 8 + 16 + 16 + 1 + 1 + 3;

pub fn snapshot_to_bytes(snap: &chip8_core::Snapshot) -> Vec<u8> {
    let mut b = Vec::with_capacity(SNAP_HEADER_LEN + SNAP_PAYLOAD_LEN);
    b.extend_from_slice(&SNAP_MAGIC);
    b.extend_from_slice(&SNAP_CODEC_VERSION.to_le_bytes());
    b.extend_from_slice(&(SNAP_PAYLOAD_LEN as u32).to_le_bytes());
    b.extend_from_slice(&*snap.mem);
    b.extend_from_slice(&snap.v);
    b.extend_from_slice(&snap.i.to_le_bytes());
    b.extend_from_slice(&snap.pc.to_le_bytes());
    for &s in &snap.stack {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b.push(snap.sp);
    b.push(snap.dt);
    b.push(snap.st);
    for plane in snap.display_buf.iter() {
        b.extend_from_slice(plane);
    }
    b.push(snap.display_hires as u8);
    b.push(snap.display_plane);
    b.extend_from_slice(&snap.keys.to_le_bytes());
    b.extend_from_slice(&snap.rng_state.to_le_bytes());
    b.extend_from_slice(&snap.flags);
    b.extend_from_slice(&snap.audio_buf);
    b.push(snap.audio_pitch);
    b.push(snap.halted as u8);
    match snap.key_wait {
        chip8_core::KeyWait::None => {
            b.push(0);
            b.push(0);
            b.push(0);
        }
        chip8_core::KeyWait::WaitPress(r) => {
            b.push(1);
            b.push(r);
            b.push(0);
        }
        chip8_core::KeyWait::WaitRelease(r, k) => {
            b.push(2);
            b.push(r);
            b.push(k);
        }
    }
    debug_assert_eq!(b.len(), SNAP_HEADER_LEN + SNAP_PAYLOAD_LEN);
    b
}

pub fn snapshot_from_bytes(bytes: &[u8]) -> Result<chip8_core::Snapshot, String> {
    if bytes.len() < SNAP_HEADER_LEN {
        return Err(format!(
            "snapshot header truncated: got {} bytes, need {SNAP_HEADER_LEN}",
            bytes.len()
        ));
    }
    if bytes[..SNAP_MAGIC.len()] != SNAP_MAGIC {
        return Err("snapshot magic does not match the CHIP-8 state codec".into());
    }
    let version = u16::from_le_bytes(
        bytes[SNAP_MAGIC.len()..SNAP_MAGIC.len() + 2]
            .try_into()
            .expect("fixed version slice"),
    );
    if version != SNAP_CODEC_VERSION {
        return Err(format!(
            "unsupported CHIP-8 snapshot codec version {version}; expected {SNAP_CODEC_VERSION}"
        ));
    }
    let declared_len = u32::from_le_bytes(
        bytes[SNAP_MAGIC.len() + 2..SNAP_HEADER_LEN]
            .try_into()
            .expect("fixed length slice"),
    ) as usize;
    if declared_len != SNAP_PAYLOAD_LEN {
        return Err(format!(
            "snapshot declares payload length {declared_len}; expected {SNAP_PAYLOAD_LEN}"
        ));
    }
    let expected_len = SNAP_HEADER_LEN + declared_len;
    if bytes.len() != expected_len {
        return Err(format!(
            "snapshot length mismatch: got {}, expected {expected_len}",
            bytes.len()
        ));
    }

    let bytes = &bytes[SNAP_HEADER_LEN..];
    let mut pos = 0usize;
    macro_rules! take {
        ($n:expr) => {{
            let n: usize = $n;
            if pos + n > bytes.len() {
                return Err(format!("snapshot truncated at offset {pos} (need {n})"));
            }
            let s = &bytes[pos..pos + n];
            pos += n;
            s
        }};
    }
    macro_rules! u16le {
        () => {
            u16::from_le_bytes(take!(2).try_into().unwrap())
        };
    }
    macro_rules! bool8 {
        ($name:literal) => {{
            match take!(1)[0] {
                0 => false,
                1 => true,
                value => {
                    return Err(format!(
                        "snapshot {} boolean is non-canonical: {value}",
                        $name
                    ));
                }
            }
        }};
    }

    let mut mem = Box::new([0u8; SNAP_MEM]);
    mem.copy_from_slice(take!(SNAP_MEM));

    let mut v = [0u8; 16];
    v.copy_from_slice(take!(16));

    let i = u16le!();
    let pc = u16le!();

    let mut stack = [0u16; 16];
    for s in &mut stack {
        *s = u16le!();
    }

    let sp = take!(1)[0];
    if sp > 16 {
        return Err(format!(
            "snapshot stack pointer {sp} exceeds stack depth 16"
        ));
    }
    let dt = take!(1)[0];
    let st = take!(1)[0];

    let mut display_buf = Box::new([[0u8; SNAP_BUF]; 2]);
    for plane in display_buf.iter_mut() {
        plane.copy_from_slice(take!(SNAP_BUF));
    }

    let display_hires = bool8!("display_hires");
    let display_plane = take!(1)[0];
    if display_plane > 3 {
        return Err(format!(
            "snapshot display plane mask {display_plane} exceeds 3"
        ));
    }
    let keys = u16le!();
    let rng_state = u64::from_le_bytes(take!(8).try_into().unwrap());
    if rng_state == 0 {
        return Err("snapshot RNG state must be non-zero".into());
    }

    let mut flags = [0u8; 16];
    flags.copy_from_slice(take!(16));

    let mut audio_buf = [0u8; 16];
    audio_buf.copy_from_slice(take!(16));

    let audio_pitch = take!(1)[0];
    let halted = bool8!("halted");

    let kw_tag = take!(1)[0];
    let kw_a = take!(1)[0];
    let kw_b = take!(1)[0];
    let key_wait = match kw_tag {
        0 if kw_a == 0 && kw_b == 0 => chip8_core::KeyWait::None,
        0 => return Err("snapshot none key-wait state has non-zero payload".into()),
        1 if kw_a < 16 && kw_b == 0 => chip8_core::KeyWait::WaitPress(kw_a),
        1 => return Err("snapshot wait-press state has an invalid register or payload".into()),
        2 if kw_a < 16 && kw_b < 16 => chip8_core::KeyWait::WaitRelease(kw_a, kw_b),
        2 => return Err("snapshot wait-release state has an invalid register or key".into()),
        t => return Err(format!("unknown KeyWait tag {t}")),
    };
    if pos != bytes.len() {
        return Err(format!(
            "snapshot decoder consumed {pos} of {} payload bytes",
            bytes.len()
        ));
    }

    Ok(chip8_core::Snapshot {
        mem,
        v,
        i,
        pc,
        stack,
        sp,
        dt,
        st,
        display_buf,
        display_hires,
        display_plane,
        keys,
        rng_state,
        flags,
        audio_buf,
        audio_pitch,
        halted,
        key_wait,
    })
}
