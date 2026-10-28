//! `cargo xtask record-fixture` (04 §13.4, 02 §3.8, 00 §2) : relaie ses arguments à l'exemple
//! `record_fixture` de l'adapter PISTE, en boîte noire.
//!
//! `xtask` ne dépend d'aucun crate `lf-*` (R5) : il lance `cargo run -p lf-adapter-piste
//! --example record_fixture --features record -- <arguments>`. Les identifiants passent par
//! l'environnement hérité (`LF_PISTE_CLIENT_ID`, `LF_PISTE_CLIENT_SECRET`), jamais par les
//! arguments ; l'exemple lit, contrôle et écrit tout le reste.

/// Arguments de `cargo` qui lancent l'exemple avec `args`, tels quels et dans l'ordre.
pub fn example_command(args: &[String]) -> Vec<String> {
    let mut command: Vec<String> = [
        "run",
        "--quiet",
        "--locked",
        "--package",
        "lf-adapter-piste",
        "--example",
        "record_fixture",
        "--features",
        "record",
        "--",
    ]
    .map(str::to_owned)
    .into();
    command.extend(args.iter().cloned());
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_fixture_forwards_to_example() {
        let args = ["--plan", "plan.toml", "--only", "inconnu/"].map(str::to_owned);
        assert_eq!(
            example_command(&args).join(" "),
            "run --quiet --locked --package lf-adapter-piste --example record_fixture --features record -- --plan plan.toml --only inconnu/"
        );
        assert_eq!(example_command(&[]).last().map(String::as_str), Some("--"));
    }
}
