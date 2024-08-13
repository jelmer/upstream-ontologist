use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

fn generate_upstream_tests(testdata_dir: &Path, dest_path: &Path) -> std::io::Result<()> {
    let mut w = fs::File::create(dest_path)?;

    writeln!(w, "use std::path::PathBuf;")?;
    writeln!(w, "use pretty_assertions::{{assert_eq, assert_ne}};")?;

    for entry in fs::read_dir(testdata_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.is_dir() {
            // Get the directory name to use in the test function name
            let dir_name = path.file_name().unwrap().to_str().unwrap();

            // Generate a test function for this directory
            writeln!(w, "#[test]")?;
            writeln!(w, "fn test_{}() {{", dir_name.replace(['-', '.'], "_"))?;
            writeln!(w, "    let dir = PathBuf::from(\"testdata/{}\");", dir_name)?;
            writeln!(w, "    let expected: serde_yaml::Value = serde_yaml::from_str(&std::fs::read_to_string(dir.join(\"expected.yaml\")).unwrap()).unwrap();")?;
            writeln!(w, "    let actual: serde_yaml::Value = serde_yaml::to_value(crate::get_upstream_info(&dir, Some(true), Some(false), Some(false), Some(false)).unwrap()).unwrap();")?;
            writeln!(w, "    assert_eq!(actual, expected);")?;
            writeln!(w, "}}")?;
            writeln!(w)?;
        }
    }

    Ok(())
}

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("upstream_tests.rs");

    generate_upstream_tests(Path::new("testdata"), &dest_path).unwrap();
}
