use std::path::PathBuf;

pub const SHADER_ARTIFACTS: &[&str] = &[
    "raygen.spv",
    "miss.spv",
    "closesthit.spv",
    "intersection.spv",
];

pub fn shader_output_dir() -> PathBuf {
    PathBuf::from(env!("OUT_DIR")).join("shaders")
}

pub fn compiled_shader_artifacts() -> Vec<PathBuf> {
    SHADER_ARTIFACTS
        .iter()
        .map(|artifact| shader_output_dir().join(artifact))
        .collect()
}
