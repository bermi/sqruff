use crate::commands::FixArgs;
use crate::commands::Format;
use crate::linter;
use sqruff_lib::core::config::FluffConfig;
use std::path::Path;

pub(crate) fn run_fix(
    args: FixArgs,
    config: FluffConfig,
    ignorer: impl Fn(&Path) -> bool + Send + Sync,
    collect_parse_errors: bool,
) -> i32 {
    let FixArgs { paths, format } = args;
    let mut linter = match linter(config, format, collect_parse_errors) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let result = match linter.lint_paths(paths, true, &ignorer) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("{}", e.value);
            return 1;
        }
    };

    if !result.has_violations() {
        println!("{} files processed, nothing to fix.", result.len());
        0
    } else {
        let any_unfixable_errors = result.has_unfixable_violations();
        let files = result.len();

        for mut file in result {
            if !file.has_fixes() {
                continue;
            }
            let path = std::mem::take(&mut file.path);
            let fixed = file.fix_string();
            if let Err(e) = std::fs::write(&path, fixed) {
                eprintln!("Error writing {}: {}", path, e);
            }
        }

        linter.formatter_mut().unwrap().completion_message(files);

        any_unfixable_errors as i32
    }
}

pub(crate) fn run_fix_stdin(
    config: FluffConfig,
    format: Format,
    collect_parse_errors: bool,
) -> i32 {
    let read_in = crate::stdin::read_std_in().unwrap();

    let linter = match linter(config, format, collect_parse_errors) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let result = match linter.lint_string(&read_in, None, true) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("{}", e.value);
            return 1;
        }
    };

    let has_unfixable_errors = result.has_unfixable_violations();

    println!("{}", result.fix_string());

    // if all fixable violations are fixable, return 0 else return 1
    has_unfixable_errors as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use std::thread::sleep;
    use std::time::Duration;
    use tempfile::NamedTempFile;

    fn ignore_none(_: &Path) -> bool {
        false
    }

    #[test]
    fn run_fix_does_not_panic_on_read_only_file() {
        use std::os::unix::fs::PermissionsExt;

        let mut tmp = NamedTempFile::new().unwrap();
        // Write SQL with a fixable violation (leading whitespace violates LT02)
        writeln!(tmp, "     SELECT 1").unwrap();
        tmp.flush().unwrap();
        let tmp = tmp.into_temp_path();
        let path = tmp.to_path_buf();

        // Make the file read-only
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();

        let args = FixArgs {
            paths: vec![path.clone()],
            format: Format::Human,
        };
        let config = FluffConfig::default();

        // This should not panic — it should handle the permission error gracefully
        run_fix(args, config, ignore_none, true);

        // Verify the file was not modified (write should have failed)
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(
            contents.starts_with("     "),
            "file should be unchanged since write was denied"
        );

        // Restore write permissions so cleanup can remove the temp file
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[test]
    fn run_fix_does_not_update_mtime_when_no_changes() {
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "SELECT 1 FROM").unwrap();
        tmp.flush().unwrap();
        let tmp = tmp.into_temp_path();
        let path = tmp.to_path_buf();

        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        sleep(Duration::from_secs(1));

        let args = FixArgs {
            paths: vec![path.clone()],
            format: Format::Human,
        };
        let config = FluffConfig::default();
        run_fix(args, config, ignore_none, true);

        let after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(before, after);
    }
}
