use knowcraft_core::error::Error;

#[test]
fn error_display_roundtrip() {
    let e = Error::Store("db corrupt".into());
    assert!(e.to_string().contains("db corrupt"));
}

#[test]
fn config_defaults_to_knowledge_dir() {
    let cfg = knowcraft_core::config::Config::default();
    assert!(cfg.data_dir.to_string_lossy().ends_with("Knowledge"));
}
