use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const SHADERS: &[(&str, &str)] = &[
    ("raygen.hlsl", "raygen_main"),
    ("miss.hlsl", "miss_main"),
    ("closesthit.hlsl", "closest_hit_main"),
    ("intersection.hlsl", "intersection_main"),
];

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let shader_dir = manifest_dir.join("shaders");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("shaders");

    println!("cargo:rerun-if-changed={}", shader_dir.display());
    fs::create_dir_all(&out_dir).expect("create shader output directory");

    let dxc =
        find_dxc().expect("Unable to find dxc.exe. Install the Vulkan SDK or add dxc to PATH.");

    for (file_name, entry_point) in SHADERS {
        let source = shader_dir.join(file_name);
        let output = out_dir.join(Path::new(file_name).with_extension("spv"));

        let status = Command::new(&dxc)
            .args([
                OsStr::new("-spirv"),
                OsStr::new("-T"),
                OsStr::new("lib_6_3"),
                OsStr::new("-E"),
                OsStr::new(entry_point),
                OsStr::new("-fspv-target-env=vulkan1.3"),
                OsStr::new("-I"),
                shader_dir.as_os_str(),
                OsStr::new("-Fo"),
                output.as_os_str(),
                source.as_os_str(),
            ])
            .status()
            .expect("failed to launch dxc");

        if !status.success() {
            panic!("dxc failed to compile {}", source.display());
        }
    }
}

fn find_dxc() -> Option<PathBuf> {
    if let Some(vulkan_sdk) = env::var_os("VULKAN_SDK") {
        let candidate = PathBuf::from(vulkan_sdk)
            .join("Bin")
            .join(if cfg!(windows) { "dxc.exe" } else { "dxc" });
        if candidate.exists() {
            return Some(candidate);
        }
    }

    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|dir| dir.join(if cfg!(windows) { "dxc.exe" } else { "dxc" }))
            .find(|candidate| candidate.exists())
    })
}
