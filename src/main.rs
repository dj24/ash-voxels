fn main() {
    if let Err(error) = ash_voxels::app::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
