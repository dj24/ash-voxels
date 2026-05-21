use std::path::PathBuf;

pub const SHADER_ARTIFACTS: &[&str] = &[
    "raygen.spv",
    "miss.spv",
    "closesthit.spv",
    "intersection.spv",
    "terrain_gen.spv",
    "coarse_depth_prepass.spv",
    "coarse_depth_debug.spv",
];

pub fn shader_output_dir() -> PathBuf {
    PathBuf::from(env!("OUT_DIR")).join("shaders")
}

pub fn compiled_shader_artifact(name: &str) -> PathBuf {
    shader_output_dir().join(name)
}

pub fn compiled_shader_artifacts() -> Vec<PathBuf> {
    SHADER_ARTIFACTS
        .iter()
        .map(|artifact| compiled_shader_artifact(artifact))
        .collect()
}
