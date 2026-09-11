//! Does the quoting actually hold, against a real shell?
//!
//! Every other test of `ssh::command::quote` asserts the *shape* of the
//! string it produces - that it starts with a quote, that an embedded quote
//! is closed and reopened. That proves the function does what its author
//! meant, which is not the same as proving a shell agrees. Four of this
//! project's seven CRITICAL findings were command injection (`AUDIT_REPORT.md`
//! S-002 through S-008), so the property worth pinning is the one that
//! actually matters: **a value handed to `quote` and interpolated into a
//! command arrives at the far end byte-for-byte, whatever is in it.**
//!
//! So these tests run `sh` and compare. `sh` stands in for the login shell on
//! a Node, which is what `SshSession::execute_command` hands its string to.
//! On a machine with no `sh` on PATH they skip rather than fail - a Windows
//! developer without Git Bash should not see a red suite for it, and CI runs
//! on Linux where it always exists.

use std::process::Command;

use vibessh_lib::ssh::command::quote;

/// The corpus. Each entry is something that changes a command's meaning when
/// it reaches a shell unquoted, with a note on what it would do.
const HOSTILE: &[(&str, &str)] = &[
    ("$(id)", "command substitution"),
    ("`id`", "backtick command substitution"),
    ("${HOME}", "parameter expansion"),
    ("$HOME", "bare parameter expansion"),
    ("a; rm -rf /", "command separator"),
    ("a && rm -rf /", "conditional chain"),
    ("a || true", "conditional chain"),
    ("a | tee /etc/passwd", "pipeline"),
    ("a > /etc/passwd", "output redirection"),
    ("a >> /etc/passwd", "appending redirection"),
    ("a < /etc/shadow", "input redirection"),
    ("a & disown", "backgrounding"),
    ("'", "a lone single quote - the one that breaks naive quoting"),
    ("''", "two single quotes"),
    ("'; rm -rf /; '", "quote break-out with a command"),
    ("\"", "double quote"),
    ("\\", "a lone backslash"),
    ("\\'", "escaped quote"),
    ("*", "glob"),
    ("?", "single-character glob"),
    ("[a-z]", "bracket glob"),
    ("~", "tilde expansion"),
    ("~root", "user tilde expansion"),
    ("!!", "history expansion"),
    ("#comment", "comment introducer"),
    ("\t", "tab"),
    ("  spaced  out  ", "leading, embedded and trailing spaces"),
    ("(subshell)", "subshell"),
    ("{a,b}", "brace expansion"),
    ("$'\\n'", "ANSI-C quoting"),
    ("\u{1b}[2J\u{1b}[H", "ANSI escape sequences"),
    ("\u{7}", "a bell character"),
    ("café ☕ 日本語", "multi-byte UTF-8"),
    ("\u{202e}gnp.exe", "a right-to-left override"),
    ("--flag", "something that looks like an option"),
    ("-rf", "something that looks like a dangerous option"),
];

fn sh_available() -> bool {
    Command::new("sh").arg("-c").arg("exit 0").status().map(|status| status.success()).unwrap_or(false)
}

/// Runs `script` through a real shell, as a *file* rather than as `sh -c`.
///
/// This matters, and cost an hour to learn: on Windows the shell is MSYS's,
/// and an argument passed to it goes through Windows argv quoting and then
/// MSYS's own re-parsing, which eats a backslash. A value of `\\` came back
/// as `\` and looked exactly like a quoting bug. It is not - it is the
/// harness losing a byte before the shell ever sees it.
///
/// Writing the script to a file removes that layer entirely, and is also
/// closer to the real thing: `SshSession::execute_command` transmits the
/// command string over the wire for the remote shell to interpret, it does
/// not hand it to a local process as argv.
fn run_script(script: &str) -> std::process::Output {
    let path = std::env::temp_dir().join(format!("vibessh-shell-test-{}.sh", uuid::Uuid::new_v4()));
    std::fs::write(&path, script).expect("writing the script should succeed");
    let output = Command::new("sh").arg(&path).output().expect("sh should run");
    let _ = std::fs::remove_file(&path);
    output
}

/// Runs `printf '%s' <quoted>` and returns what the shell passed to `printf`
/// as its argument.
///
/// `printf '%s'` rather than `echo`: `echo`'s handling of a leading `-` and
/// of backslashes is famously shell-dependent, and this test is about the
/// argument, not about `echo`.
fn what_the_shell_sees(value: &str) -> String {
    let script = format!("printf '%s' {}", quote(value));
    let output = run_script(&script);
    assert!(output.status.success(), "sh failed on {script:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).expect("output should be valid UTF-8")
}

#[test]
fn a_quoted_value_reaches_the_far_end_unchanged() {
    if !sh_available() {
        eprintln!("skipping: no `sh` on PATH");
        return;
    }
    for (value, what) in HOSTILE {
        assert_eq!(&what_the_shell_sees(value), value, "quoting failed to neutralize {what}: {value:?}");
    }
}

/// The complement, and the more important half: not merely that the value
/// survives, but that the shell never *did* anything with it. If a
/// substitution ran, its result would appear here instead of the literal.
#[test]
fn no_hostile_value_is_ever_evaluated() {
    if !sh_available() {
        eprintln!("skipping: no `sh` on PATH");
        return;
    }
    // A marker that cannot occur by accident, written into a file the
    // injected command would have to create for the test to notice.
    let scratch = std::env::temp_dir().join(format!("vibessh-injection-{}", uuid::Uuid::new_v4()));
    let marker = scratch.to_string_lossy().replace('\\', "/");

    for (value, what) in HOSTILE {
        // Build an argument that *would* create the marker file if the shell
        // evaluated any part of it.
        let payload = format!("{value}$(touch '{marker}')`touch '{marker}'`");
        let script = format!("printf '%s' {} >/dev/null", quote(&payload));
        assert!(run_script(&script).status.success(), "sh failed for {what}");
        assert!(!std::path::Path::new(&marker).exists(), "a substitution ran for {what}: {value:?}");
    }
    let _ = std::fs::remove_file(&marker);
}

/// Quoting composes: several hostile values in one command line stay
/// separate arguments, none of them able to reach into another. This is the
/// shape every real call site has - `docker create -e K=V --name N image cmd`
/// is half a dozen interpolations in one string.
#[test]
fn several_quoted_values_in_one_command_stay_separate_arguments() {
    if !sh_available() {
        eprintln!("skipping: no `sh` on PATH");
        return;
    }
    let values: Vec<&str> = HOSTILE.iter().map(|(value, _)| *value).collect();
    let args: Vec<String> = values.iter().map(|value| quote(value)).collect();
    // `printf '%s\n'` repeats its format for every remaining argument, so
    // this prints one line per argument - which is exactly the question:
    // did the shell split them the way the caller intended?
    let script = format!("printf '%s\\n' {}", args.join(" "));
    let output = run_script(&script);
    let seen: Vec<&str> = std::str::from_utf8(&output.stdout).unwrap().split('\n').collect();

    for (index, value) in values.iter().enumerate() {
        // A value containing a newline would legitimately span two lines;
        // none of the corpus does except the ANSI-C quoting case, which is
        // the literal four characters `$'\n'`.
        assert_eq!(seen.get(index).copied(), Some(*value), "argument {index} came through as something else");
    }
}

/// The corpus above is a list of things somebody thought of. This is the same
/// property over arbitrary input.
///
/// Newlines and null bytes are excluded, and for different reasons. A newline
/// is excluded because the output could not then be split back into arguments
/// unambiguously, and the test would be measuring itself. A null byte is
/// excluded because this harness cannot *carry* one at all - `Command::arg`
/// builds a C string, so it would fail here regardless of what quoting does
/// with it. Both are refused upstream by `command::reject_newlines`, which is
/// asserted directly below rather than left implied.
mod properties {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        // Deliberately fewer cases than the default: every one of these
        // spawns a real process, and 256 shells per run would make the suite
        // noticeably slower for a property that saturates long before that.
        #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

        #[test]
        fn any_value_survives_a_real_shell_intact(value in "[^\\n\\r\\x00]{0,60}") {
            prop_assume!(sh_available());
            prop_assert_eq!(what_the_shell_sees(&value), value);
        }
    }
}

/// The characters the property above cannot cover, covered directly.
///
/// A null byte matters more than it looks. The command string reaches the
/// remote shell as a C string, so a NUL inside it truncates the command
/// there - and it cannot be quoted around, only refused.
#[test]
fn line_structure_and_null_bytes_are_refused_before_quoting() {
    use vibessh_lib::ssh::command::reject_newlines;

    for (value, what) in [
        ("a\nb", "a newline"),
        ("a\rb", "a carriage return"),
        ("a\0b", "a null byte"),
        ("\0", "a lone null byte"),
    ] {
        assert!(reject_newlines(value, "a field").is_err(), "accepted {what}");
    }
    assert!(reject_newlines("a normal $value with 'quotes'", "a field").is_ok());
}
