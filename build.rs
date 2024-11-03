use quote::{format_ident, quote};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn generate_upstream_tests(testdata_dir: &Path, dest_path: &Path) -> std::io::Result<()> {
    let mut w = fs::File::create(dest_path)?;

    write!(
        w,
        "{}",
        quote! {
            use std::path::PathBuf;
            use pretty_assertions::assert_eq;
        }
    )?;

    for entry in fs::read_dir(testdata_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.is_dir() {
            // Get the directory name to use in the test function name
            let dir_name = path.file_name().unwrap().to_str().unwrap();
            let fn_name = format_ident!("test_{}", dir_name.replace(['.', '-'], "_"));

            let test = quote! {
                #[tokio::test]
                async fn #fn_name() {
                    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata").join(#dir_name);
                    let expected: serde_yaml::Value = serde_yaml::from_reader(std::fs::File::open(dir.join("expected.yaml")).unwrap()).unwrap();
                    let actual: serde_yaml::Value = serde_yaml::to_value(crate::get_upstream_info(&dir, Some(true), Some(false), Some(false), Some(false)).await.unwrap()).unwrap();
                    assert_eq!(expected, actual);
                }
            };

            writeln!(w, "{}", test)?;
        }
    }

    Ok(())
}

fn generate_readme_tests(testdata_dir: &Path, dest_path: &Path) -> std::io::Result<()> {
    let mut w = fs::File::create(dest_path)?;

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    write!(
        w,
        "{}",
        quote! {
            use std::path::PathBuf;
            use pretty_assertions::assert_eq;
            use crate::readme::{description_from_readme_md, description_from_readme_rst, description_from_readme_plain};
        }
    )?;

    for entry in fs::read_dir(testdata_dir).unwrap() {
        let entry = entry.unwrap();
        let path = manifest_dir.join(entry.path());

        if path.is_dir() {
            // Get the directory name to use in the test function name
            let dir_name = entry.file_name().to_str().unwrap().to_string();

            if path.join("README.md").exists() {
                let fn_name = format_ident!("test_{}_readme_md", dir_name.replace(['.', '-'], "_"));
                let test = quote! {
                    #[test]
                    fn #fn_name() {
                        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("readme_tests").join(#dir_name);
                        let readme_md = std::fs::read_to_string(path.join("README.md")).unwrap();
                        let expected_description = if path.join("description").exists() {
                            Some(std::fs::read_to_string(path.join("description")).unwrap())
                        } else {
                            None
                        };
                        let (actual_description, actual_md) = description_from_readme_md(&readme_md).unwrap();
                        let actual_md = serde_yaml::to_value(actual_md).unwrap();
                        assert_eq!(actual_description, expected_description);
                        if path.join("expected.yaml").exists() {
                            let expected_md: serde_yaml::Value = serde_yaml::from_reader(std::fs::File::open(path.join("expected.yaml")).unwrap()).unwrap();
                            assert_eq!(actual_md, expected_md);
                        }
                    }
                };
                write!(w, "{}", test)?;
            } else if path.join("README.rst").exists() {
                let fn_name =
                    format_ident!("test_{}_readme_rst", dir_name.replace(['.', '-'], "_"));

                let test = quote! {
                    #[test]
                    fn #fn_name() {
                        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("readme_tests").join(#dir_name);
                        let readme_rst = std::fs::read_to_string(path.join("README.rst")).unwrap();
                        let expected_description = if path.join("description").exists() {
                            Some(std::fs::read_to_string(path.join("description")).unwrap())
                        } else {
                            None
                        };
                        let (actual_description, actual_md) = description_from_readme_rst(&readme_rst).unwrap();
                        let actual_md = serde_yaml::to_value(actual_md).unwrap();
                        assert_eq!(actual_description, expected_description);
                        if path.join("expected.yaml").exists() {
                            let expected_md: serde_yaml::Value = serde_yaml::from_reader(std::fs::File::open(path.join("expected.yaml")).unwrap()).unwrap();
                            assert_eq!(actual_md, expected_md);
                        }
                    }
                };
                write!(w, "{}", test)?;
            } else {
                let fn_name =
                    format_ident!("test_{}_readme_plain", dir_name.replace(['.', '-'], "_"));

                let test = quote! {
                    #[test]
                    fn #fn_name() {
                        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("readme_tests").join(#dir_name);
                        let readme_plain = std::fs::read_to_string(path.join("README")).unwrap();
                        let expected_description = if path.join("description").exists() {
                            Some(std::fs::read_to_string(path.join("description")).unwrap())
                        } else {
                            None
                        };
                        let (actual_description, actual_md) = description_from_readme_plain(&readme_plain).unwrap();
                        let actual_md = serde_yaml::to_value(actual_md).unwrap();
                        assert_eq!(actual_description, expected_description);
                        if path.join("expected.yaml").exists() {
                            let expected_md: serde_yaml::Value = serde_yaml::from_reader(std::fs::File::open(path.join("expected.yaml")).unwrap()).unwrap();
                            assert_eq!(actual_md, expected_md);
                        }
                    }
                };
                write!(w, "{}", test)?;
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
