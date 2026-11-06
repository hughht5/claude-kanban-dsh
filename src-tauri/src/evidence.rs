//! The card-tester's screenshots (card #101). The tester names each shot it took in its final
//! report, on a line of its own: `EVIDENCE: before|after <full path> | <caption>`. When it stops,
//! `agents.rs` finds the card, lets go of the board lock and calls `prepare`: each named file is
//! checked and copied into the data folder, `evidence/<run>/<stamp>-<n>-<kind>.png`, as a new
//! file. `<stamp>` is the digits of the verdict's `at`, so a resumed tester (the same agent id)
//! files each run's shots beside the older ones. The run then goes on the card in the same write
//! as the tester's verdict (the card field `evidence`, in `kanban_store`).
//!
//! Only PNGs from the temp folder (the tester's scratchpad) are taken, never a file from the data
//! folder, a network path or a repo checkout. A copy never writes over a file. `tools/cdp.mjs`
//! writes the test app's build commit into each shot (a `tEXt` chunk, `kanban-build`); it goes on
//! the shot as `build`.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The most a shot may be.
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// The most shots a run takes; the 7th and later are refused.
pub const MAX_SHOTS: usize = 6;
/// The longest a caption is kept, in characters.
pub const MAX_CAPTION: usize = 200;
/// The longest a build name is kept, in characters.
pub const MAX_BUILD: usize = 64;
/// The `tEXt` keyword `tools/cdp.mjs` writes the test app's build commit under.
pub const BUILD_KEYWORD: &[u8] = b"kanban-build";
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const KINDS: [&str; 2] = ["before", "after"];

/// One `EVIDENCE:` line, as the tester wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// "before" or "after" (lower case); anything else is refused by `prepare`.
    pub kind: String,
    pub path: String,
    /// "" when it gave none.
    pub caption: String,
}

/// What `prepare` took from a report.
#[derive(Debug, Default)]
pub struct Prepared {
    /// The shots copied in, as the run's `shots` list holds them.
    pub shots: Vec<Value>,
    /// Why each other named file wasn't taken: "<file name>: <reason>", with secrets masked.
    pub refused: Vec<String>,
    /// Where the shots went, relative to the data folder: "evidence/<run>".
    pub folder: String,
}

/// The `EVIDENCE:` lines in an agent's report, in order. Bold (`**EVIDENCE:**`), a list marker,
/// backticks or quotes round the path, either slash and spaces in the path are all fine; the
/// caption is optional. Any other line, and an `EVIDENCE:` line with no path, is ignored.
pub fn parse(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let l = raw.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '*' | '_' | '`' | '-' | '>'));
        let Some(head) = l.get(..9) else { continue };
        if !head.eq_ignore_ascii_case("EVIDENCE:") {
            continue;
        }
        // The rest, without the bold that closed "**EVIDENCE:**" or the whole line.
        let rest = l[9..].trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '*' | '_')).trim_end().trim_end_matches('*').trim_end();
        let (word, rest) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let kind = word.trim_matches(|c: char| matches!(c, '*' | '`' | ':' | '_')).to_ascii_lowercase();
        // A Windows path never holds "|", so the first one ends it.
        let (path, caption) = rest.split_once('|').unwrap_or((rest, ""));
        let path = path.trim().trim_matches(|c: char| matches!(c, '`' | '"' | '*')).trim();
        // No path, or the prompt's own pattern ("<full path>") quoted back: not a shot.
        if path.is_empty() || path.contains(['<', '>']) {
            continue;
        }
        let caption = caption.trim().trim_matches(|c: char| matches!(c, '`' | '*')).trim();
        out.push(Line { kind, path: path.to_string(), caption: caption.to_string() });
    }
    out
}

/// The run's folder name: the agent id with only letters, digits, `-` and `_` kept.
pub fn run_folder(run: &str) -> String {
    run.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect()
}

/// The start of each of a run's file names: the digits of its `at`, so
/// "2026-11-06T03:15:00.123Z" gives "20261106031500123".
pub fn run_stamp(at: &str) -> String {
    at.chars().filter(char::is_ascii_digit).collect()
}

/// Reads, checks and copies the shots `text` names into `<data_dir>/evidence/<run>`, each named
/// `<stamp>-<n>-<kind>.png` with the digits of `at` (the verdict's own `at`). It takes no store
/// and no lock, so the board is never held while files are read or written. Each file must be a
/// local, absolute path whose real path is inside `temp_dir` and outside `data_dir`, a PNG by its
/// first bytes and its `.png` name, and at most 5 MB; a run takes at most 6 shots.
pub fn prepare(data_dir: &Path, temp_dir: &Path, run: &str, at: &str, text: &str) -> Prepared {
    let lines = parse(text);
    let folder = run_folder(run);
    let stamp = run_stamp(at);
    let mut p = Prepared { folder: format!("evidence/{folder}"), ..Prepared::default() };
    if lines.is_empty() {
        return p;
    }
    let refuse = |p: &mut Prepared, path: &str, why: &str| p.refused.push(kanban_store::mask_secrets(&format!("{}: {why}", file_name(path))));
    if folder.is_empty() {
        for l in &lines {
            refuse(&mut p, &l.path, "the run has no agent id to file it under");
        }
        return p;
    }
    let Ok(temp) = std::fs::canonicalize(temp_dir) else {
        for l in &lines {
            refuse(&mut p, &l.path, "the app can't read its temp folder");
        }
        return p;
    };
    let data = std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
    for l in &lines {
        match take(&p, l, &temp, &data, data_dir, &folder, &stamp) {
            Ok(shot) => p.shots.push(shot),
            Err(why) => refuse(&mut p, &l.path, &why),
        }
    }
    p
}

/// One line's shot, or why it isn't taken.
fn take(p: &Prepared, l: &Line, temp: &Path, data: &Path, data_dir: &Path, folder: &str, stamp: &str) -> Result<Value, String> {
    if !KINDS.contains(&l.kind.as_str()) {
        return Err(format!("\"{}\" isn't a kind of shot (before or after)", l.kind));
    }
    if p.shots.len() >= MAX_SHOTS {
        return Err(format!("a run takes at most {MAX_SHOTS} shots"));
    }
    let named = local_path(&l.path)?;
    let real = std::fs::canonicalize(&named).map_err(|_| "not found".to_string())?;
    if real.starts_with(data) {
        return Err("it is in the app's data folder".into());
    }
    if !real.starts_with(temp) {
        return Err("it isn't in the temp folder (only shots from the tester's scratchpad are taken)".into());
    }
    let png_name = real.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_ascii_lowercase().ends_with(".png"));
    if !png_name {
        return Err("its name doesn't end in .png".into());
    }
    // Read once, at most one byte over the limit; those bytes are checked and written.
    let mut bytes = Vec::new();
    std::fs::File::open(&real)
        .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("can't read it: {e}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("it is over 5 MB".into());
    }
    let (width, height) = png_size(&bytes).ok_or("not a PNG")?;
    let n = p.shots.len() + 1;
    let name = if stamp.is_empty() { format!("{n}-{}.png", l.kind) } else { format!("{stamp}-{n}-{}.png", l.kind) };
    let dir = data_dir.join("evidence").join(folder);
    std::fs::create_dir_all(&dir).map_err(|e| format!("can't make {}: {e}", slashes(&dir)))?;
    let dest = dir.join(&name);
    let mut f = match std::fs::OpenOptions::new().write(true).create_new(true).open(&dest) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(format!("{folder}/{name} is already there, and a shot is never written over")),
        Err(e) => return Err(format!("can't write {name}: {e}")),
    };
    f.write_all(&bytes).and_then(|_| f.flush()).map_err(|e| format!("couldn't write all of {name}: {e}"))?;
    let caption: String = l.caption.chars().take(MAX_CAPTION).collect();
    let mut shot = json!({
        "n": n,
        "kind": l.kind,
        "caption": kanban_store::mask_secrets(&caption),
        "bytes": bytes.len(),
        "width": width,
        "height": height,
        "file": format!("evidence/{folder}/{name}"),
        "path": slashes(&dest),
    });
    if let Some(build) = png_build(&bytes) {
        shot["build"] = json!(build);
    }
    Ok(shot)
}

/// The build commit `tools/cdp.mjs` wrote into a shot: the text of its first sound `tEXt` chunk
/// whose keyword is `kanban-build`, with only `0-9 a-z A-Z . _ -` kept, at most 64 of them. It
/// walks the chunks after the signature by their lengths and stops at IEND or at a chunk that
/// runs past the end; a `tEXt` chunk whose CRC is wrong is skipped. None when there is none.
pub fn png_build(bytes: &[u8]) -> Option<String> {
    if bytes.get(..8)? != PNG_SIGNATURE {
        return None;
    }
    let mut pos = 8usize;
    loop {
        let head = bytes.get(pos..pos.checked_add(8)?)?;
        let len = usize::try_from(u32::from_be_bytes([head[0], head[1], head[2], head[3]])).ok()?;
        let kind = &head[4..8];
        let end = pos.checked_add(8)?.checked_add(len)?;
        // The data and its CRC must both be there; a chunk that runs past the end ends the walk.
        let data = bytes.get(pos + 8..end)?;
        let crc = bytes.get(end..end.checked_add(4)?)?;
        if kind == b"IEND" {
            return None;
        }
        if kind == b"tEXt" && crc32(&bytes[pos + 4..end]) == u32::from_be_bytes([crc[0], crc[1], crc[2], crc[3]]) {
            if let Some(text) = data.strip_prefix(BUILD_KEYWORD).and_then(|t| t.strip_prefix(&[0u8])) {
                let build: String = text.iter().map(|&b| char::from(b)).filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')).take(MAX_BUILD).collect();
                if !build.is_empty() {
                    return Some(build);
                }
            }
        }
        pos = end + 4;
    }
}

/// The PNG chunk CRC (CRC-32, as in zlib) of `bytes`.
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// `p` as a local, absolute path: a drive letter and a slash. A network (UNC) or device path,
/// and a relative one, are refused.
fn local_path(p: &str) -> Result<PathBuf, String> {
    let b = p.as_bytes();
    if b.len() >= 2 && matches!(b[0], b'\\' | b'/') && matches!(b[1], b'\\' | b'/') {
        return Err("a network or device path isn't taken; name a local file".into());
    }
    if !(b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/')) {
        return Err("not a full path (it starts C:\\)".into());
    }
    Ok(PathBuf::from(p.replace('/', "\\")))
}

/// The width and height in a PNG's IHDR header, if `bytes` start like a PNG.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// The last part of a path as the tester wrote it, either slash.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(path)
}

/// A path with forward slashes.
fn slashes(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A run's `at`, and the digits its files are named with.
    const AT: &str = "2026-11-06T03:15:00.123Z";
    const STAMP: &str = "20261106031500123";

    /// A small PNG's start: the signature and an IHDR header of `w` by `h`, then `extra` bytes.
    fn png(w: u32, h: u32, extra: usize) -> Vec<u8> {
        let mut b = PNG_SIGNATURE.to_vec();
        b.extend_from_slice(&13u32.to_be_bytes());
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        b.extend(std::iter::repeat(7u8).take(extra));
        b
    }

    /// A root with a `temp` folder and a `data` folder beside it.
    struct Dirs {
        _root: tempfile::TempDir,
        temp: PathBuf,
        data: PathBuf,
        other: PathBuf,
    }

    fn dirs() -> Dirs {
        let root = tempfile::tempdir().unwrap();
        let (temp, data, other) = (root.path().join("temp"), root.path().join("data"), root.path().join("other"));
        for d in [&temp, &data, &other] {
            std::fs::create_dir_all(d).unwrap();
        }
        Dirs { _root: root, temp, data, other }
    }

    fn put(dir: &Path, name: &str, bytes: &[u8]) -> String {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p.to_string_lossy().to_string()
    }

    fn one(d: &Dirs, line: &str) -> Prepared {
        prepare(&d.data, &d.temp, "agent-1", AT, line)
    }

    #[test]
    fn evidence_lines_are_read_in_their_usual_forms() {
        let text = "Tested it.\n\
            EVIDENCE: before C:\\Users\\x\\Temp\\s\\1.png | the board before\n\
            **EVIDENCE:** after `C:/Users/x/Temp/s/after shot.png` | the chip, after\n\
            **EVIDENCE: after C:\\Users\\Hugh Smith\\Temp\\two words.png**\n\
            - evidence: BEFORE \"C:/t/q.png\"|caption with no spaces round the bar\n\
            EVIDENCE: during C:/t/x.png | an unknown kind\n\
            EVIDENCE:\n\
            EVIDENCE: after\n\
            EVIDENCE: before|after <full path> | <short caption>\n\
            Name each shot on its own line: EVIDENCE: after C:/t/mid.png\n\
            EVIDENCES are good\n\
            CARD: #101\nVERDICT: PASS";
        let lines = parse(text);
        let l = |kind: &str, path: &str, caption: &str| Line { kind: kind.into(), path: path.into(), caption: caption.into() };
        assert_eq!(
            lines,
            vec![
                l("before", "C:\\Users\\x\\Temp\\s\\1.png", "the board before"),
                l("after", "C:/Users/x/Temp/s/after shot.png", "the chip, after"),
                l("after", "C:\\Users\\Hugh Smith\\Temp\\two words.png", ""),
                l("before", "C:/t/q.png", "caption with no spaces round the bar"),
                l("during", "C:/t/x.png", "an unknown kind"),
            ]
        );
        assert!(parse("no evidence here\nCARD: #1").is_empty());
        assert!(parse("").is_empty());
    }

    #[test]
    fn a_png_from_the_temp_folder_is_copied_byte_for_byte() {
        let d = dirs();
        let bytes = png(800, 600, 100);
        let src = put(&d.temp, "after.png", &bytes);
        let fwd = src.replace('\\', "/");
        let p = prepare(&d.data, &d.temp, "a1b2-c_3", AT, &format!("EVIDENCE: after `{fwd}` | the chip\nCARD: #5\nVERDICT: PASS"));
        assert!(p.refused.is_empty(), "{:?}", p.refused);
        assert_eq!(p.folder, "evidence/a1b2-c_3");
        assert_eq!(p.shots.len(), 1);
        let s = &p.shots[0];
        assert_eq!((s["n"].as_u64(), s["kind"].as_str(), s["caption"].as_str()), (Some(1), Some("after"), Some("the chip")));
        assert_eq!((s["width"].as_u64(), s["height"].as_u64(), s["bytes"].as_u64()), (Some(800), Some(600), Some(bytes.len() as u64)));
        assert_eq!(s["file"], format!("evidence/a1b2-c_3/{STAMP}-1-after.png"));
        let path = s["path"].as_str().unwrap();
        assert!(!path.contains('\\') && path.ends_with(&format!("/data/evidence/a1b2-c_3/{STAMP}-1-after.png")), "{path}");
        assert_eq!(std::fs::read(d.data.join("evidence").join("a1b2-c_3").join(format!("{STAMP}-1-after.png"))).unwrap(), bytes);
        // No build chunk in the file: no "build" key.
        assert!(s.get("build").is_none(), "{s}");
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        // The source is left as it was.
        assert_eq!(std::fs::read(&src).unwrap(), bytes);
    }

    #[test]
    fn shots_are_numbered_in_order_and_never_written_over() {
        let d = dirs();
        let first = png(10, 20, 5);
        let a = put(&d.temp, "a.png", &first);
        let b = put(&d.temp, "b.png", &png(30, 40, 5));
        let text = format!("EVIDENCE: before {a} | before\nEVIDENCE: after {b} | after");
        let p = prepare(&d.data, &d.temp, "run7", AT, &text);
        assert!(p.refused.is_empty(), "{:?}", p.refused);
        let names: Vec<&str> = p.shots.iter().map(|s| s["file"].as_str().unwrap()).collect();
        assert_eq!(names, [format!("evidence/run7/{STAMP}-1-before.png"), format!("evidence/run7/{STAMP}-2-after.png")]);
        // A second copy to the same name (the same run and `at`) is refused, and the first file
        // stays as it was.
        std::fs::write(&a, png(99, 99, 50)).unwrap();
        let again = prepare(&d.data, &d.temp, "run7", AT, &format!("EVIDENCE: before {a} | again"));
        assert!(again.shots.is_empty());
        assert_eq!(again.refused, vec![format!("a.png: run7/{STAMP}-1-before.png is already there, and a shot is never written over")]);
        assert_eq!(std::fs::read(d.data.join(format!("evidence/run7/{STAMP}-1-before.png"))).unwrap(), first);
    }

    #[test]
    fn a_resumed_tester_keeps_every_runs_shots() {
        // SendMessage resumes a tester with its agent id, so its second run has the same folder;
        // its own `at` keeps its files apart from the first run's.
        let d = dirs();
        let first = png(10, 20, 5);
        let a = put(&d.temp, "a.png", &first);
        let p1 = prepare(&d.data, &d.temp, "agent-r", "2026-11-06T03:15:00.123Z", &format!("EVIDENCE: after {a} | first run"));
        assert!(p1.refused.is_empty(), "{:?}", p1.refused);
        let second = png(30, 40, 5);
        std::fs::write(&a, &second).unwrap();
        let b = put(&d.temp, "b.png", &png(50, 60, 5));
        let p2 = prepare(&d.data, &d.temp, "agent-r", "2026-11-06T03:40:02.007Z", &format!("EVIDENCE: before {a} | second run\nEVIDENCE: after {b} | second run"));
        assert!(p2.refused.is_empty(), "{:?}", p2.refused);
        assert_eq!((p1.folder.as_str(), p2.folder.as_str()), ("evidence/agent-r", "evidence/agent-r"));
        assert_eq!(p1.shots[0]["file"], "evidence/agent-r/20261106031500123-1-after.png");
        let names: Vec<&str> = p2.shots.iter().map(|s| s["file"].as_str().unwrap()).collect();
        assert_eq!(names, ["evidence/agent-r/20261106034002007-1-before.png", "evidence/agent-r/20261106034002007-2-after.png"]);
        // Both runs' files are there, each as it was copied.
        assert_eq!(std::fs::read(d.data.join("evidence/agent-r/20261106031500123-1-after.png")).unwrap(), first);
        assert_eq!(std::fs::read(d.data.join("evidence/agent-r/20261106034002007-1-before.png")).unwrap(), second);
        assert_eq!(run_stamp("2026-11-06T03:15:00.123Z"), "20261106031500123");
        assert_eq!(run_stamp("no digits"), "");
    }

    #[test]
    fn files_that_arent_pngs_are_refused() {
        let d = dirs();
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        let gif = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff\x21\xf9\x04\x01\x00\x00\x00\x00";
        for (name, bytes) in [("fake.png", b"just some text, not a picture".as_slice()), ("photo.png", &jpeg[..]), ("anim.png", &gif[..]), ("short.png", &PNG_SIGNATURE[..])] {
            let path = put(&d.temp, name, bytes);
            let p = one(&d, &format!("EVIDENCE: after {path} | x"));
            assert!(p.shots.is_empty(), "{name}");
            assert_eq!(p.refused, vec![format!("{name}: not a PNG")]);
        }
        // A real PNG with another name.
        let path = put(&d.temp, "shot.jpg", &png(5, 5, 0));
        assert_eq!(one(&d, &format!("EVIDENCE: after {path}")).refused, vec!["shot.jpg: its name doesn't end in .png"]);
        // Upper case is still a .png.
        let path = put(&d.temp, "SHOT.PNG", &png(5, 5, 0));
        assert_eq!(one(&d, &format!("EVIDENCE: after {path}")).shots.len(), 1);
    }

    #[test]
    fn a_shot_over_5_mb_is_refused() {
        let d = dirs();
        let big = png(4000, 4000, MAX_BYTES as usize);
        let path = put(&d.temp, "big.png", &big);
        let p = one(&d, &format!("EVIDENCE: after {path}"));
        assert!(p.shots.is_empty());
        assert_eq!(p.refused, vec!["big.png: it is over 5 MB"]);
        // Exactly 5 MB is fine.
        let edge = png(10, 10, 0);
        let edge = [edge.clone(), vec![0u8; MAX_BYTES as usize - edge.len()]].concat();
        let path = put(&d.temp, "edge.png", &edge);
        assert_eq!(one(&d, &format!("EVIDENCE: after {path}")).shots.len(), 1);
    }

    #[test]
    fn only_local_full_paths_inside_the_temp_folder_are_taken() {
        let d = dirs();
        let outside = put(&d.other, "out.png", &png(5, 5, 0));
        let cases = [
            ("\\\\server\\share\\a.png", "a.png: a network or device path isn't taken; name a local file"),
            ("//server/share/b.png", "b.png: a network or device path isn't taken; name a local file"),
            ("\\\\?\\C:\\x\\c.png", "c.png: a network or device path isn't taken; name a local file"),
            ("shots\\d.png", "d.png: not a full path (it starts C:\\)"),
            ("\\Temp\\e.png", "e.png: not a full path (it starts C:\\)"),
            ("C:f.png", "C:f.png: not a full path (it starts C:\\)"),
            (outside.as_str(), "out.png: it isn't in the temp folder (only shots from the tester's scratchpad are taken)"),
        ];
        for (path, why) in cases {
            let p = one(&d, &format!("EVIDENCE: after {path} | x"));
            assert!(p.shots.is_empty(), "{path}");
            assert_eq!(p.refused, vec![why.to_string()], "{path}");
        }
        // Missing, and a way out of the temp folder with "..".
        let missing = d.temp.join("nope.png").to_string_lossy().to_string();
        assert_eq!(one(&d, &format!("EVIDENCE: after {missing}")).refused, vec!["nope.png: not found"]);
        let sneaky = format!("{}\\..\\other\\out.png", d.temp.display());
        assert!(one(&d, &format!("EVIDENCE: after {sneaky}")).refused[0].contains("isn't in the temp folder"));
        // An unknown kind, before any file is read.
        let ok = put(&d.temp, "ok.png", &png(5, 5, 0));
        assert_eq!(one(&d, &format!("EVIDENCE: during {ok}")).refused, vec!["ok.png: \"during\" isn't a kind of shot (before or after)"]);
        // Nothing was copied for any of them.
        assert!(!d.data.join("evidence").exists());
    }

    #[test]
    fn a_file_in_the_data_folder_is_refused_even_when_that_folder_is_in_temp() {
        let root = tempfile::tempdir().unwrap();
        let temp = root.path().to_path_buf();
        let data = temp.join(".claude-kanban-dsh");
        std::fs::create_dir_all(data.join("evidence").join("old")).unwrap();
        let inside = put(&data.join("evidence").join("old"), "1-after.png", &png(5, 5, 0));
        let p = prepare(&data, &temp, "new", AT, &format!("EVIDENCE: after {inside}"));
        assert!(p.shots.is_empty());
        assert_eq!(p.refused, vec!["1-after.png: it is in the app's data folder"]);
        // A file beside it in temp is still taken.
        let fine = put(&temp, "fine.png", &png(5, 5, 0));
        assert_eq!(prepare(&data, &temp, "new", AT, &format!("EVIDENCE: after {fine}")).shots.len(), 1);
    }

    #[test]
    fn a_run_takes_at_most_six_shots() {
        let d = dirs();
        let mut text = String::new();
        for i in 1..=8 {
            let path = put(&d.temp, &format!("s{i}.png"), &png(i, i, 0));
            text.push_str(&format!("EVIDENCE: {} {path} | shot {i}\n", if i % 2 == 1 { "before" } else { "after" }));
        }
        let p = one(&d, &text);
        assert_eq!(p.shots.len(), MAX_SHOTS);
        assert_eq!(p.refused, vec!["s7.png: a run takes at most 6 shots", "s8.png: a run takes at most 6 shots"]);
        assert_eq!(p.shots[5]["file"], format!("evidence/agent-1/{STAMP}-6-after.png"));
        assert!(!d.data.join(format!("evidence/agent-1/{STAMP}-7-before.png")).exists());
        // A refused line doesn't use up a place.
        let d = dirs();
        let bad = put(&d.temp, "bad.png", b"text");
        let good = put(&d.temp, "good.png", &png(5, 5, 0));
        let mut text = format!("EVIDENCE: before {bad}\n");
        for _ in 0..6 {
            text.push_str(&format!("EVIDENCE: after {good}\n"));
        }
        let p = one(&d, &text);
        assert_eq!((p.shots.len(), p.refused.len()), (6, 1));
        assert_eq!(p.shots[0]["n"], 1);
    }

    #[test]
    fn captions_are_cut_and_masked_and_so_are_refusals() {
        let d = dirs();
        let path = put(&d.temp, "a.png", &png(5, 5, 0));
        let p = one(&d, &format!("EVIDENCE: after {path} | the Password box shows eyJhbGciOiJIUzI1NiJ9.e30.abc and sk_live_123"));
        let cap = p.shots[0]["caption"].as_str().unwrap();
        assert_eq!(cap, "the Pass word box shows [hidden] and [hidden]");
        assert!(!kanban_store::looks_secret(cap));
        let long = "x".repeat(500);
        let path = put(&d.temp, "b.png", &png(5, 5, 0));
        let p = prepare(&d.data, &d.temp, "agent-2", AT, &format!("EVIDENCE: after {path} | {long}"));
        assert_eq!(p.shots[0]["caption"].as_str().unwrap().chars().count(), MAX_CAPTION);
        // A refusal's file name is masked too, so its log line is taken.
        let p = one(&d, "EVIDENCE: after C:/nowhere/my-password-eyJhbGciOiJIUzI1NiJ9.png");
        assert_eq!(p.refused.len(), 1);
        assert!(!kanban_store::looks_secret(&p.refused[0]), "{}", p.refused[0]);
    }

    #[test]
    fn the_run_folder_keeps_only_safe_characters() {
        assert_eq!(run_folder("a1b2-c_3"), "a1b2-c_3");
        assert_eq!(run_folder("..\\..\\x/y:z *?"), "xyz");
        assert_eq!(run_folder(""), "");
        let d = dirs();
        let path = put(&d.temp, "a.png", &png(5, 5, 0));
        let p = prepare(&d.data, &d.temp, "../..", AT, &format!("EVIDENCE: after {path}"));
        assert!(p.shots.is_empty());
        assert_eq!(p.refused, vec!["a.png: the run has no agent id to file it under"]);
        // No lines, no work: nothing is made.
        let p = prepare(&d.data, &d.temp, "r", AT, "CARD: #1\nVERDICT: PASS");
        assert!(p.shots.is_empty() && p.refused.is_empty());
        assert!(!d.data.join("evidence").exists());
    }

    /// A PNG chunk: its length, kind, data and CRC.
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut c = (data.len() as u32).to_be_bytes().to_vec();
        c.extend_from_slice(kind);
        c.extend_from_slice(data);
        c.extend_from_slice(&crc32(&c[4..]).to_be_bytes());
        c
    }

    /// A whole small PNG (signature, IHDR, `middle`, a stand-in IDAT, IEND).
    fn png_with(middle: &[Vec<u8>]) -> Vec<u8> {
        let mut ihdr = 4u32.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&3u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut b = PNG_SIGNATURE.to_vec();
        b.extend(chunk(b"IHDR", &ihdr));
        for m in middle {
            b.extend_from_slice(m);
        }
        b.extend(chunk(b"IDAT", &[0x78, 0x9c, 3, 0, 0, 0, 0, 1]));
        b.extend(chunk(b"IEND", &[]));
        b
    }

    fn text(keyword: &str, value: &[u8]) -> Vec<u8> {
        let mut d = keyword.as_bytes().to_vec();
        d.push(0);
        d.extend_from_slice(value);
        chunk(b"tEXt", &d)
    }

    #[test]
    fn the_crc_is_the_pngs() {
        // Every PNG ends with this IEND chunk; its CRC is a fixed value.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn a_shot_carries_the_build_cdp_wrote_into_it() {
        let ok = png_with(&[text("kanban-build", b"0321fbe1a2b3")]);
        assert_eq!(png_build(&ok).as_deref(), Some("0321fbe1a2b3"));
        assert_eq!(png_size(&ok), Some((4, 3)));
        // After other chunks too, and the first kanban-build one wins.
        let later = png_with(&[text("Software", b"Chromium"), text("kanban-build", b"abc-1.2_x"), text("kanban-build", b"other")]);
        assert_eq!(png_build(&later).as_deref(), Some("abc-1.2_x"));
        // Only safe characters, at most 64.
        let odd = png_with(&[text("kanban-build", b"ab c/../\\d\x00e<script>\xff")]);
        assert_eq!(png_build(&odd).as_deref(), Some("abc..descript"));
        let long = png_with(&[text("kanban-build", "x".repeat(200).as_bytes())]);
        assert_eq!(png_build(&long).map(|b| b.len()), Some(MAX_BUILD));
        // "unknown" is what cdp.mjs writes when it couldn't read the build: kept as it is.
        assert_eq!(png_build(&png_with(&[text("kanban-build", b"unknown")])).as_deref(), Some("unknown"));

        // It lands on the shot as "build".
        let d = dirs();
        let path = put(&d.temp, "built.png", &ok);
        let p = one(&d, &format!("EVIDENCE: after {path} | built"));
        assert!(p.refused.is_empty(), "{:?}", p.refused);
        assert_eq!(p.shots[0]["build"], "0321fbe1a2b3");
        assert_eq!(std::fs::read(d.data.join(format!("evidence/agent-1/{STAMP}-1-after.png"))).unwrap(), ok, "copied byte for byte");
    }

    #[test]
    fn a_shot_with_no_build_has_no_build_key() {
        for bytes in [
            png_with(&[]),
            png_with(&[text("Software", b"Chromium")]),
            png_with(&[text("kanban-builds", b"abc")]),
            png_with(&[text("kanban-build", b"")]),
            png_with(&[text("kanban-build", b" /\\ <>")]),
            // A chunk with no keyword separator.
            png_with(&[chunk(b"tEXt", b"kanban-buildabc")]),
        ] {
            assert_eq!(png_build(&bytes), None);
            let d = dirs();
            let path = put(&d.temp, "plain.png", &bytes);
            let p = one(&d, &format!("EVIDENCE: after {path}"));
            assert_eq!(p.shots.len(), 1, "{:?}", p.refused);
            assert!(p.shots[0].get("build").is_none(), "{}", p.shots[0]);
        }
        // A chunk after IEND isn't read.
        let mut after_end = png_with(&[]);
        after_end.extend(text("kanban-build", b"late"));
        assert_eq!(png_build(&after_end), None);
        // Not a PNG at all.
        assert_eq!(png_build(b"GIF89a"), None);
        assert_eq!(png_build(&[]), None);
    }

    #[test]
    fn a_broken_chunk_is_ignored_safely() {
        let good = text("kanban-build", b"abc123");
        // A wrong CRC: that chunk is skipped, and a sound one after it is still read.
        let mut bad_crc = good.clone();
        let n = bad_crc.len();
        bad_crc[n - 1] ^= 0xFF;
        assert_eq!(png_build(&png_with(&[bad_crc.clone()])), None);
        assert_eq!(png_build(&png_with(&[bad_crc, text("kanban-build", b"def456")])).as_deref(), Some("def456"));
        // A length that runs past the end, the largest length, and one that ends inside the
        // CRC: the walk stops, with no panic.
        for len in [u32::MAX, 0x7FFF_FFFF, 1000, (good.len() - 12 + 2) as u32] {
            let mut broken = good.clone();
            broken[..4].copy_from_slice(&len.to_be_bytes());
            let mut bytes = PNG_SIGNATURE.to_vec();
            bytes.extend_from_slice(&broken);
            assert_eq!(png_build(&bytes), None, "{len}");
            // The same broken chunk ahead of a good one: the good one isn't reached.
            let mut both = bytes.clone();
            both.extend_from_slice(&good);
            assert_eq!(png_build(&both), None, "{len}");
        }
        // Cut off anywhere: never a panic, and a build only once the whole chunk is there.
        let whole = png_with(&[good.clone()]);
        let end_of_text = 8 + 25 + good.len();
        for cut in 0..whole.len() {
            let b = png_build(&whole[..cut]);
            assert_eq!(b.is_some(), cut >= end_of_text, "{cut}");
        }
        // The old test PNG (an IHDR then filler bytes) has no build, and is still taken.
        assert_eq!(png_build(&png(5, 5, 100)), None);
    }
}
