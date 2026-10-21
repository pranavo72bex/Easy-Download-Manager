//! Joins MP4 files' tracks into one, as dm does with a video's picture and
//! sound: `cargo run -p dm-core --example join -- out.mp4 video.mp4 audio.m4a`.

fn main() {
    let args: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    let Some((out, inputs)) = args.split_first().filter(|(_, i)| !i.is_empty()) else {
        eprintln!("usage: join OUT.mp4 INPUT…");
        std::process::exit(2);
    };
    let started = std::time::Instant::now();
    match dm_core::mp4mux::join(inputs, out) {
        Ok(bytes) => println!("{bytes} bytes in {:?}", started.elapsed()),
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1);
        }
    }
}
