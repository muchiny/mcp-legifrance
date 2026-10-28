//! Arguments de la ligne de commande de `cargo xtask`, testables hors du binaire.

use std::ffi::OsString;

/// Convertit les arguments (programme exclu par l'appelant) en `String`.
///
/// `env::args()` panique sur un argument qui n'est pas de l'UTF-8 ; `args_os` puis cette
/// conversion explicite rendent une erreur claire, qui ne recopie pas l'argument (il pourrait
/// porter un secret).
///
/// # Errors
///
/// Un message fixe si un argument n'est pas de l'UTF-8 valide.
pub fn utf8_args(args: impl IntoIterator<Item = OsString>) -> Result<Vec<String>, String> {
    args.into_iter()
        .map(|arg| arg.into_string().map_err(|_| "argument is not valid UTF-8".to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_arguments_are_kept_in_order() {
        let args = ["check-arch", "--plan", "é"].map(OsString::from);
        assert_eq!(utf8_args(args).unwrap(), ["check-arch", "--plan", "é"]);
        assert_eq!(utf8_args(Vec::<OsString>::new()).unwrap(), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn an_argument_that_is_not_utf8_is_refused_without_being_shown() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(b"--token=\xffsecret".to_vec());
        let error = utf8_args([OsString::from("record-fixture"), bad]).unwrap_err();
        assert_eq!(error, "argument is not valid UTF-8");
    }
}
