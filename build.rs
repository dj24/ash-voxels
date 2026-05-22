use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct ShaderSpec<'a> {
    file_name: &'a str,
    entry_point: &'a str,
    profile: &'a str,
}

const SHADERS: &[ShaderSpec<'_>] = &[
    ShaderSpec {
        file_name: "ray_query.hlsl",
        entry_point: "ray_query_main",
        profile: "cs_6_5",
    },
    ShaderSpec {
        file_name: "terrain_gen.hlsl",
        entry_point: "terrain_gen_main",
        profile: "cs_6_3",
    },
    ShaderSpec {
        file_name: "coarse_depth_prepass.hlsl",
        entry_point: "coarse_depth_prepass_main",
        profile: "vs_6_3",
    },
    ShaderSpec {
        file_name: "coarse_depth_debug.hlsl",
        entry_point: "coarse_depth_debug_main",
        profile: "cs_6_3",
    },
];

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let shader_dir = manifest_dir.join("shaders");
    let terrain_source = manifest_dir.join("src").join("terrain.rs");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("shaders");
    let terrain_grid_side = read_terrain_constant(&terrain_source, "TERRAIN_GRID_SIDE");
    let terrain_grid_height_layers =
        read_terrain_constant(&terrain_source, "TERRAIN_GRID_HEIGHT_LAYERS");

    println!("cargo:rerun-if-changed={}", shader_dir.display());
    println!("cargo:rerun-if-changed={}", terrain_source.display());
    fs::create_dir_all(&out_dir).expect("create shader output directory");

    let dxc =
        find_dxc().expect("Unable to find dxc.exe. Install the Vulkan SDK or add dxc to PATH.");

    for shader in SHADERS {
        let source = shader_dir.join(shader.file_name);
        let output = out_dir.join(Path::new(shader.file_name).with_extension("spv"));
        let terrain_grid_side_define = format!("TERRAIN_GRID_SIDE_VALUE={terrain_grid_side}");
        let terrain_grid_height_layers_define =
            format!("TERRAIN_GRID_HEIGHT_LAYERS_VALUE={terrain_grid_height_layers}");

        let status = Command::new(&dxc)
            .args([
                OsStr::new("-spirv"),
                OsStr::new("-T"),
                OsStr::new(shader.profile),
                OsStr::new("-E"),
                OsStr::new(shader.entry_point),
                OsStr::new("-fspv-target-env=vulkan1.3"),
                OsStr::new("-I"),
                shader_dir.as_os_str(),
                OsStr::new("-D"),
                OsStr::new(&terrain_grid_side_define),
                OsStr::new("-D"),
                OsStr::new(&terrain_grid_height_layers_define),
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

fn read_terrain_constant(terrain_source: &Path, constant_name: &str) -> u32 {
    let source = fs::read_to_string(terrain_source)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", terrain_source.display()));

    let line = source
        .lines()
        .find(|line| line.contains(&format!("pub const {constant_name}")))
        .unwrap_or_else(|| {
            panic!(
                "failed to find {constant_name} constant in {}",
                terrain_source.display(),
            )
        });

    let value = line
        .split('=')
        .nth(1)
        .map(str::trim)
        .and_then(|value| value.strip_suffix(';'))
        .unwrap_or_else(|| {
            panic!(
                "failed to parse {constant_name} constant from line `{line}` in {}",
                terrain_source.display()
            )
        });

    value.parse::<u32>().unwrap_or_else(|error| {
        panic!(
            "failed to parse {constant_name} value `{value}` in {}: {error}",
            terrain_source.display()
        )
    })
}
