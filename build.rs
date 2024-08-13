use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn generate_upstream_tests(testdata_dir: &Path, dest_path: &Path) -> std::io::Result<()> {
    let mut w = fs::File::create(dest_path)?;

    writeln!(w, "use std::path::PathBuf;")?;
    writeln!(w, "use pretty_assertions::{{assert_eq, assert_ne}};")?;

    let manifest_dir = env!("CARGO_MANIFEST_DIR");

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
            writeln!(w, "    let expected: serde_yaml::Value = serde_yaml::from_str(include_str!(\"{}/testdata/{}/expected.yaml\")).unwrap();", manifest_dir, dir_name)?;
            writeln!(w, "    let actual: serde_yaml::Value = serde_yaml::to_value(crate::get_upstream_info(&dir, Some(true), Some(false), Some(false), Some(false)).unwrap()).unwrap();")?;
            writeln!(w, "    assert_eq!(actual, expected);")?;
            writeln!(w, "}}")?;
            writeln!(w)?;
        }
    }

    Ok(())
}

fn generate_readme_tests(testdata_dir: &Path, dest_path: &Path) -> std::io::Result<()> {
    let mut w = fs::File::create(dest_path)?;

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    write!(w, "use std::path::PathBuf;")?;
    write!(w, "use pretty_assertions::{{assert_eq, assert_ne}};")?;
    write!(w, "use crate::readme::{{description_from_readme_md, description_from_readme_rst, description_from_readme_plain}};")?;

    for entry in fs::read_dir(testdata_dir).unwrap() {
        let entry = entry.unwrap();
        let path = manifest_dir.join(entry.path());

        if path.is_dir() {
            // Get the directory name to use in the test function name
            let dir_name = entry.file_name().to_str().unwrap().to_string();

            if path.join("README.md").exists() {
                writeln!(w, "#[test]")?;
                writeln!(
                    w,
                    "fn test_{}_readme_md() {{",
                    dir_name.replace(['-', '.'], "_")
                )?;
                writeln!(
                    w,
                    "    let readme_md = include_str!(\"{}/README.md\");",
                    path.display()
                )?;
                if path.join("description").exists() {
                    writeln!(
                        w,
                        "    let expected_description = Some(include_str!(\"{}/description\"));",
                        path.display()
                    )?;
                } else {
                    writeln!(w, "    let expected_description: Option<&str> = None;")?;
                }

                writeln!(w, "    let (actual_description, actual_md) = description_from_readme_md(readme_md).unwrap();")?;
                writeln!(
                    w,
                    "    let actual_md = serde_yaml::to_value(actual_md).unwrap();"
                )?;
                writeln!(
                    w,
                    "    assert_eq!(actual_description.as_deref(), expected_description);"
                )?;
                if path.join("expected.yaml").exists() {
                    writeln!(w, "    let expected_md: serde_yaml::Value = serde_yaml::from_str(include_str!(\"{}/expected.yaml\")).unwrap();", path.display())?;
                    writeln!(w, "    assert_eq!(actual_md, expected_md);")?;
                }
                writeln!(w, "}}")?;
                writeln!(w)?;
            } else if path.join("README.rst").exists() {
                writeln!(w, "#[test]")?;
                writeln!(
                    w,
                    "fn test_{}_readme_rst() {{",
                    dir_name.replace(['-', '.'], "_")
                )?;
                writeln!(
                    w,
                    "    let readme_rst = include_str!(\"{}/README.rst\");",
                    path.display()
                )?;
                if path.join("description").exists() {
                    writeln!(
                        w,
                        "    let expected_description = Some(include_str!(\"{}/description\"));",
                        path.display()
                    )?;
                } else {
                    writeln!(w, "    let expected_description: Option<&str> = None;")?;
                }
                writeln!(w, "    let (actual_description, actual_md) = description_from_readme_rst(readme_rst).unwrap();")?;
                writeln!(
                    w,
                    "    let actual_md = serde_yaml::to_value(actual_md).unwrap();"
                )?;
                writeln!(
                    w,
                    "    assert_eq!(actual_description.as_deref(), expected_description);"
                )?;
                if path.join("expected.yaml").exists() {
                    writeln!(w, "    let expected_md: serde_yaml::Value = serde_yaml::from_str(include_str!(\"{}/expected.yaml\")).unwrap();", path.display())?;
                    writeln!(w, "    assert_eq!(actual_md, expected_md);")?;
                }

                writeln!(w, "}}")?;
                writeln!(w)?;
            } else {
                writeln!(w, "#[test]")?;
                writeln!(
                    w,
                    "fn test_{}_readme_plain() {{",
                    dir_name.replace(['-', '.'], "_")
                )?;
                writeln!(
                    w,
                    "    let readme_plain = include_str!(\"{}/README\");",
                    path.display()
                )?;
                if path.join("description").exists() {
                    writeln!(
                        w,
                        "    let expected_description = Some(include_str!(\"{}/description\"));",
                        path.display()
                    )?;
                } else {
                    writeln!(w, "    let expected_description: Option<&str> = None;")?;
                }
                writeln!(w, "    let (actual_description, actual_md) = description_from_readme_plain(readme_plain).unwrap();")?;
                writeln!(
                    w,
                    "    let actual_md = serde_yaml::to_value(actual_md).unwrap();"
                )?;
                writeln!(
                    w,
                    "    assert_eq!(actual_description.as_deref(), expected_description);"
                )?;
                if path.join("expected.yaml").exists() {
                    writeln!(w, "    let expected_md: serde_yaml::Value = serde_yaml::from_str(include_str!(\"{}/expected.yaml\")).unwrap();", path.display())?;
                    writeln!(w, "    assert_eq!(actual_md, expected_md);")?;
                }
                writeln!(w, "}}")?;
                writeln!(w)?;
            }
        }
    }

    Ok(())
}

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    generate_upstream_tests(
        Path::new("testdata"),
        &Path::new(&out_dir).join("upstream_tests.rs"),
    )
    .unwrap();

    generate_readme_tests(
        Path::new("readme_tests"),
        &Path::new(&out_dir).join("readme_tests.rs"),
    )
    .unwrap();
}
