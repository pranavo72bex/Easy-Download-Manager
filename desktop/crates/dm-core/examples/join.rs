//! Joins MP4 (or WebM) files' tracks into one, as dm does with a video's
//! picture and sound: `cargo run -p dm-core --example join -- out.mp4
//! video.mp4 audio.m4a`, or `out.webm` with WebM inputs.

fn main() {
    let args: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    let Some((out, inputs)) = args.split_first().filter(|(_, i)| !i.is_empty()) else {
        eprintln!("usage: join OUT.mp4 INPUT…");
        std::process::exit(2);
    };
    let started = std::time::Instant::now();
    let webm = out.extension().is_some_and(|e| e == "webm" || e == "mkv");
    let joined = if webm {
        dm_core::mkvmux::join(inputs, out)
    } else {
        dm_core::mp4mux::join(inputs, out)
    };
    match joined {
        Ok(bytes) => println!("{bytes} bytes in {:?}", started.elapsed()),
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1);
        }
    }
}
