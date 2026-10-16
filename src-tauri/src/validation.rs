//! Security validation utilities.
//!
//! Input sanitization for shell commands, container IDs, Kubernetes resource names,
//! and AppleScript injection prevention.

/// Banned shell metacharacters for container/VM exec commands (Fix #2)
pub fn contains_shell_injection(input: &str) -> bool {
    let banned_chars = [';', '`', '$'];
    let banned_seqs = ["&&", "||", "$(", "${", "..", ">/"];
    for ch in &banned_chars {
        if input.contains(*ch) {
            return true;
        }
    }
    for seq in &banned_seqs {
        if input.contains(seq) {
            return true;
        }
    }
    false
}

/// Stable IDs carried by Docker resource lists, rather than names/CLI options.
pub fn is_stable_container_id(id: &str) -> bool {
    (12..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Validate container_id format (hex string, 1-128 chars)
pub fn is_valid_container_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/' || c == ':'
        })
}

/// Dangerous docker run flags that could lead to container escape (Fix #10)
pub const BANNED_DOCKER_FLAGS: &[&str] = &[
    "--privileged",
    "--pid=host",
    "--pid",
    "host",
    "--net=host",
    "--network=host",
    "--cap-add=SYS_ADMIN",
    "--cap-add=ALL",
    "--security-opt=apparmor:unconfined",
    "--security-opt=seccomp:unconfined",
];

/// Whitelist of resource types allowed for deletion (Fix #5)
pub const K8S_DELETABLE_RESOURCES: &[&str] = &[
    "pods",
    "pod",
    "deployments",
    "deployment",
    "services",
    "service",
    "configmaps",
    "configmap",
    "secrets",
    "secret",
    "statefulsets",
    "statefulset",
    "daemonsets",
    "daemonset",
    "replicasets",
    "replicaset",
    "jobs",
    "job",
    "cronjobs",
    "cronjob",
    "ingresses",
    "ingress",
    "persistentvolumeclaims",
    "pvc",
    "endpoints",
    "serviceaccounts",
    "serviceaccount",
    "networkpolicies",
    "networkpolicy",
    "poddisruptionbudgets",
    "pdb",
    "horizontalpodautoscalers",
    "hpa",
];

/// Validate a Kubernetes resource name (namespace, pod, container)
/// Must be alphanumeric, hyphens, dots, underscores only.
pub fn is_valid_k8s_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
}

/// Escape a string for embedding inside AppleScript double-quoted strings (Fix #3).
/// Escapes backslashes and double quotes to prevent injection.
pub fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Maximum profile name length. The lima instance name is `colima-{profile}`,
/// and that becomes a directory name and a hostname component.
const MAX_PROFILE_NAME_LEN: usize = 63;

/// Validate a Colima profile / instance name.
///
/// A profile name reaches two dangerous places: it is pushed into argv after
/// `--profile`, and it is joined into a filesystem path under `~/.colima`.
/// So the rules here are about *safety*, not about Colima compatibility:
///
///   - must start with an alphanumeric — this is what stops `-rf` being read as
///     a flag rather than a value, and stops `.`/`..` path tricks;
///   - allows `-`, `_`, `.` inside, because Colima accepts them today
///     (underscores are accepted by Colima and only break later, at Kubernetes
///     node registration — see abiosoft/colima#745). Rejecting them here would
///     break existing users' profiles to fix a problem that isn't ours.
///   - rejects `..` outright, path separators, control characters and
///     whitespace.
///
/// Empty is rejected; call sites that treat empty as "use the default profile"
/// should use [`ensure_valid_profile`] instead.
pub fn is_valid_profile_name(name: &str) -> bool {
    is_safe_cli_name(name, MAX_PROFILE_NAME_LEN)
}

/// Maximum length for docker/lima resource names (volumes, networks, VMs).
const MAX_RESOURCE_NAME_LEN: usize = 128;

/// Validate a docker or lima resource name (volume, network, VM instance).
///
/// Same hazard as a profile name: these are pushed into argv, so a value
/// starting with `-` is read as a flag. Note this is deliberately stricter than
/// [`is_valid_k8s_name`], which permits a leading dash and is used for values
/// that are not passed positionally.
pub fn is_valid_resource_name(name: &str) -> bool {
    is_safe_cli_name(name, MAX_RESOURCE_NAME_LEN)
}

/// Shared rule behind [`is_valid_profile_name`] and [`is_valid_resource_name`].
fn is_safe_cli_name(name: &str, max_len: usize) -> bool {
    if name.is_empty() || name.len() > max_len {
        return false;
    }
    // Leading character carries the argv- and path-injection risk.
    if !name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        return false;
    }
    if name.contains("..") {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Call-site helper: validate a profile argument, accepting empty as "default".
///
/// Every command in `commands/colima.rs` treats an empty profile as the default
/// one, so folding that case in here keeps each call site to a single line and
/// removes the chance of getting the empty check wrong at one of them.
pub fn ensure_valid_profile(profile: &str) -> Result<(), String> {
    if profile.is_empty() || is_valid_profile_name(profile) {
        Ok(())
    } else {
        Err(format!(
            "Invalid profile name: {:?}. Use letters, digits, '-', '_' or '.', starting with a letter or digit.",
            profile
        ))
    }
}

/// Host output names are one directory entry, never another path or NT stream.
pub fn ensure_host_file_name(name: &str) -> Result<(), String> {
    let mut parts = std::path::Path::new(name).components();
    if !matches!(parts.next(), Some(std::path::Component::Normal(_))) || parts.next().is_some() {
        return Err("Choose a file name within the selected folder".into());
    }
    #[cfg(windows)]
    {
        let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
        if name.ends_with([' ', '.'])
            || name
                .chars()
                .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
            || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(
                "Choose an ordinary Windows file name without reserved names or alternate streams"
                    .into(),
            );
        }
    }
    Ok(())
}

/// Resolve a caller-selected output folder/name to the canonical identity used
/// by all file-export claims. Confinement is against the selected folder;
/// canonical parent/target identity also closes directory-alias claim bypasses.
pub fn resolve_host_output(
    dir: &str,
    file_name: &str,
    overwrite: bool,
) -> Result<std::path::PathBuf, String> {
    for (label, value) in [("Destination folder", dir), ("File name", file_name)] {
        if value.trim().is_empty() {
            return Err(format!("{label} must not be empty"));
        }
        if value.chars().any(|c| c.is_control()) {
            return Err(format!("{label} contains control characters"));
        }
        if value.trim_start().starts_with('-') {
            return Err(format!("{label} must not start with '-'"));
        }
    }
    ensure_host_file_name(file_name)?;
    let base = std::path::Path::new(dir);
    if !base.is_dir() {
        return Err(format!("Destination folder does not exist: {dir}"));
    }
    let candidate = base.join(file_name);
    assert_path_within(base, &candidate)?;
    if candidate.exists() && !overwrite {
        return Err(format!(
            "{} already exists. Choose another name or confirm overwriting.",
            candidate.display()
        ));
    }
    if candidate.exists() {
        candidate
            .canonicalize()
            .map_err(|e| format!("Cannot resolve destination: {e}"))
    } else {
        let parent = candidate
            .parent()
            .ok_or("Destination has no parent")?
            .canonicalize()
            .map_err(|e| format!("Cannot resolve destination folder: {e}"))?;
        Ok(parent.join(
            candidate
                .file_name()
                .ok_or("Destination has no file name")?,
        ))
    }
}

/// Callers canonicalize existing parents first. Windows comparison deliberately
/// refuses case aliases conservatively, even in a case-sensitive directory.
pub fn same_host_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    #[cfg(not(windows))]
    {
        a == b
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let a: Vec<_> = a.as_os_str().encode_wide().collect();
        let b: Vec<_> = b.as_os_str().encode_wide().collect();
        unsafe {
            windows_sys::Win32::Globalization::CompareStringOrdinal(
                a.as_ptr(),
                a.len() as i32,
                b.as_ptr(),
                b.len() as i32,
                1,
            ) == 2
        }
    }
}

/// Assert that `candidate` resolves inside `base`, defeating `..` traversal and
/// symlinks that point out of the tree.
///
/// Canonicalizes the *parent* of `candidate`, not `candidate` itself, so this
/// works for paths that do not exist yet (a file about to be written).
///
/// # Which base to pass
///
/// The base is a policy decision per operation, not a constant. Callers that
/// write to the host filesystem use:
///
/// | Operation | Base |
/// |---|---|
/// | `docker cp` container→host | The directory the user picked in the system dialog |
/// | `docker save` → TAR | The directory the user picked in the system dialog |
/// | `docker cp` host→container | No host-side confinement — it is a read. The *container* side is validated with [`is_valid_container_id`] and passed via `Command::args`, never interpolated into a shell |
///
/// Container-side paths deliberately do **not** go through this function: they
/// name a different filesystem, so canonicalizing them against a host directory
/// would be meaningless. Safety there comes from never invoking `sh -c`.
///
/// Note that [`contains_shell_injection`] is a metacharacter denylist for
/// user-typed exec commands. It is **not** a path guard and must not be
/// substituted for this function.
pub fn assert_path_within(
    base: &std::path::Path,
    candidate: &std::path::Path,
) -> Result<(), String> {
    let base = base
        .canonicalize()
        .map_err(|e| format!("Cannot resolve base directory {}: {}", base.display(), e))?;

    let resolved = if candidate.exists() {
        candidate
            .canonicalize()
            .map_err(|e| format!("Cannot resolve path {}: {}", candidate.display(), e))?
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| format!("Path has no parent: {}", candidate.display()))?;
        let parent = parent
            .canonicalize()
            .map_err(|e| format!("Cannot resolve directory {}: {}", parent.display(), e))?;
        match candidate.file_name() {
            Some(name) => parent.join(name),
            None => parent,
        }
    };

    if resolved.starts_with(&base) {
        Ok(())
    } else {
        Err(format!(
            "Path {} escapes {}",
            resolved.display(),
            base.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_parent_aliases_resolve_to_one_identity_without_changing_prior_bytes() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp")
            .join(format!(
                "output-identity-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
        std::fs::create_dir_all(root.join("nested")).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let canonical = root.canonicalize().unwrap();
        let alias = canonical.join("nested").join("..");
        let resolve = |dir: &std::path::Path, overwrite| {
            resolve_host_output(dir.to_str().unwrap(), "report 空間.json", overwrite)
        };
        let intended = canonical.join("report 空間.json");
        assert_eq!(resolve(&canonical, false).unwrap(), intended);
        assert_eq!(resolve(&alias, false).unwrap(), intended);
        std::fs::write(&intended, b"retained output").unwrap();
        assert!(resolve(&alias, false).is_err());
        assert_eq!(resolve(&alias, true).unwrap(), intended);
        assert_eq!(std::fs::read(intended).unwrap(), b"retained output");
    }

    #[test]
    fn host_output_names_cannot_choose_another_directory() {
        for name in [
            "",
            "..",
            ".",
            "../outside.tar",
            "/outside.tar",
            "nested/file.tar",
        ] {
            assert!(ensure_host_file_name(name).is_err(), "{name}");
        }
        ensure_host_file_name("archive 空間.tar").unwrap();
        #[cfg(windows)]
        for name in [
            "C:escape.tar",
            "file.tar:secret",
            "CON.tar",
            "nul",
            "LPT9.tar",
            "file.tar.",
            "file.tar ",
            "folder\\file.tar",
            "C:\\outside.tar",
        ] {
            assert!(ensure_host_file_name(name).is_err(), "{name}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn native_case_aliases_cannot_obtain_separate_destination_claims() {
        assert!(same_host_path(
            std::path::Path::new(r"C:\Folder\File.tar"),
            std::path::Path::new(r"c:\folder\file.TAR")
        ));
        assert!(!same_host_path(
            std::path::Path::new(r"C:\Folder\File.tar"),
            std::path::Path::new(r"D:\Folder\File.tar")
        ));
    }

    #[test]
    fn accepts_realistic_profile_names() {
        for name in [
            "default",
            "dev",
            "k8s-test",
            "my_profile",
            "colima.v2",
            "a",
            "test123",
        ] {
            assert!(is_valid_profile_name(name), "should accept: {name}");
        }
    }

    #[test]
    fn rejects_argv_flag_injection() {
        // The live bug: a leading dash makes colima read the value as a flag.
        for name in ["-rf", "--profile", "-", "--"] {
            assert!(!is_valid_profile_name(name), "should reject: {name}");
        }
    }

    #[test]
    fn rejects_path_traversal() {
        for name in [
            "..",
            "../etc",
            "../../etc/passwd",
            "a/../..",
            ".hidden",
            "/abs",
            "a/b",
            "a\\b",
        ] {
            assert!(!is_valid_profile_name(name), "should reject: {name}");
        }
    }

    #[test]
    fn rejects_control_and_whitespace() {
        for name in ["a\0b", "a b", " lead", "trail ", "a\tb", "a\nb"] {
            assert!(!is_valid_profile_name(name), "should reject: {name:?}");
        }
    }

    #[test]
    fn rejects_empty_and_overlong() {
        assert!(!is_valid_profile_name(""));
        assert!(!is_valid_profile_name(
            &"a".repeat(MAX_PROFILE_NAME_LEN + 1)
        ));
        assert!(is_valid_profile_name(&"a".repeat(MAX_PROFILE_NAME_LEN)));
    }

    #[test]
    fn rejects_non_ascii_lookalikes() {
        // U+2010 HYPHEN and U+FF0D FULLWIDTH HYPHEN-MINUS look like '-'.
        assert!(!is_valid_profile_name("\u{2010}rf"));
        assert!(!is_valid_profile_name("caf\u{e9}"));
    }

    #[test]
    fn ensure_valid_profile_treats_empty_as_default() {
        assert!(ensure_valid_profile("").is_ok());
        assert!(ensure_valid_profile("default").is_ok());
        assert!(ensure_valid_profile("-rf").is_err());
    }

    #[test]
    fn assert_path_within_allows_child_and_blocks_escape() {
        let base = std::env::temp_dir().join("colima-ui-validation-test");
        std::fs::create_dir_all(&base).expect("temp dir");

        // A file that does not exist yet, directly under base: allowed.
        assert!(assert_path_within(&base, &base.join("child.yaml")).is_ok());

        // Same, one real directory deeper.
        let nested = base.join("nested");
        std::fs::create_dir_all(&nested).expect("nested dir");
        assert!(assert_path_within(&base, &nested.join("ok.yaml")).is_ok());

        // Traversal out of the tree: rejected.
        let escape = base.join("..").join("..").join("etc").join("passwd");
        assert!(assert_path_within(&base, &escape).is_err());

        // Unresolvable parent fails closed rather than being waved through.
        // Callers create the directory first; a missing one means we cannot
        // prove where the write would land, so we refuse.
        let missing_parent = base.join("does-not-exist").join("file.yaml");
        assert!(assert_path_within(&base, &missing_parent).is_err());

        std::fs::remove_dir_all(&base).ok();
    }
}
