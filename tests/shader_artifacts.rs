use ash_voxels::shader_build::compiled_shader_artifacts;

#[test]
fn compiled_shader_artifacts_exist() {
    for artifact in compiled_shader_artifacts() {
        assert!(
            artifact.exists(),
            "missing compiled shader artifact: {}",
            artifact.display()
        );
    }
}
