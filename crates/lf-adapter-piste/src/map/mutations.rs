//! Mutations des fixtures de référence : aucune ne fait paniquer le décodage ni le mapping (06
//! §2.3, T-M2-06, D-23, D-24). Ce module n'existe que sous `cfg(test)`.
//!
//! Chaque nœud de chaque fixture subit tour à tour quatre mutations : remplacé par `null`, retiré
//! de son objet, changé de type, vidé (liste ou objet non vide). Le document muté passe par le
//! chemin de production ([`map_body`] : `decode` sous budget, puis le mapper) ; l'issue doit être
//! `Ok`, `Decode` ou `NotFound`, jamais une panique ni une autre erreur.

use std::panic::{AssertUnwindSafe, catch_unwind};

use lf_application::ports::CorpusError;
use serde_json::{Value, json};

use super::Diagnostics;
use super::fixtures::{ALL, Fixture, Mapped, decoded, map_body};

/// Une mutation d'un nœud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mutation {
    /// Le nœud devient `null`.
    Null,
    /// Le membre est retiré de son objet (la racine n'est pas retirée).
    Removed,
    /// Le nœud change de type : texte ⇔ nombre, booléen ⇒ texte, liste ⇔ objet, `null` ⇒ texte.
    Retyped,
    /// Une liste ou un objet non vide est vidé.
    Emptied,
}

const MUTATIONS: [Mutation; 4] =
    [Mutation::Null, Mutation::Removed, Mutation::Retyped, Mutation::Emptied];

/// Pointeur JSON de chaque nœud de `value`, racine (`""`) comprise, en préordre.
fn pointers(value: &Value, pointer: &str, out: &mut Vec<String>) {
    out.push(pointer.to_owned());
    match value {
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                pointers(item, &format!("{pointer}/{index}"), out);
            }
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                pointers(item, &format!("{pointer}/{escaped}"), out);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Le nœud de type changé.
fn retyped(node: &Value) -> Value {
    match node {
        Value::Null => json!("x"),
        Value::Bool(flag) => json!(flag.to_string()),
        Value::Number(number) => json!(number.to_string()),
        Value::String(_) => json!(7),
        Value::Array(_) => json!({}),
        Value::Object(_) => json!([]),
    }
}

/// `document` où le nœud `pointer` a subi `mutation`, `None` si elle ne s'applique pas.
fn mutated(document: &Value, pointer: &str, mutation: Mutation) -> Option<Value> {
    let mut copy = document.clone();
    let node = copy.pointer_mut(pointer).expect("pointer from the document");
    match mutation {
        Mutation::Null => *node = Value::Null,
        Mutation::Retyped => *node = retyped(node),
        Mutation::Emptied => match node {
            Value::Array(items) if !items.is_empty() => items.clear(),
            Value::Object(fields) if !fields.is_empty() => fields.clear(),
            _ => return None,
        },
        Mutation::Removed => {
            let (parent, key) = pointer.rsplit_once('/')?;
            let key = key.replace("~1", "/").replace("~0", "~");
            let fields = copy.pointer_mut(parent).expect("parent").as_object_mut()?;
            fields.remove(&key);
        }
    }
    Some(copy)
}

/// Message d'écart pour l'issue `outcome` de la mutation `label`, `None` si elle est admise :
/// `Ok`, `Decode` ou `NotFound`.
fn refused(
    label: &str,
    outcome: std::thread::Result<Result<Mapped, CorpusError>>,
) -> Option<String> {
    match outcome {
        Ok(Ok(_) | Err(CorpusError::Decode { .. } | CorpusError::NotFound { .. })) => None,
        Ok(Err(other)) => Some(format!("{label}: {other:?}")),
        Err(_) => Some(format!("{label}: panic")),
    }
}

/// Issues inattendues des mutations de `fixture` (panique ou erreur autre que `Decode` et
/// `NotFound`) et nombre de documents mutés essayés.
fn check(fixture: &Fixture) -> (Vec<String>, usize) {
    let document: Value = serde_json::from_str(&decoded(fixture, true).text).expect("JSON");
    let mut all = Vec::new();
    pointers(&document, "", &mut all);
    let mut failures = Vec::new();
    let mut tried = 0_usize;
    for pointer in &all {
        for mutation in MUTATIONS {
            let Some(body) = mutated(&document, pointer, mutation) else { continue };
            tried = tried.saturating_add(1);
            let text = body.to_string();
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                map_body(fixture.kind, &text, fixture.requested, &mut Diagnostics::default())
            }));
            failures.extend(refused(&format!("{} {pointer} {mutation:?}", fixture.name), outcome));
        }
    }
    (failures, tried)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_mutations_never_panic() {
        let mut failures = Vec::new();
        let mut tried = 0;
        for fixture in &ALL {
            let (found, count) = check(fixture);
            failures.extend(found);
            tried += count;
        }
        assert_eq!(failures, Vec::<String>::new());
        // 16 fixtures, chaque nœud par les quatre mutations qui s'y appliquent (mesure du plan 2g).
        assert_eq!(tried, 10_763);
    }

    #[test]
    fn every_mutation_has_its_effect() {
        let document = json!({"a": [1, "t", true, null, {"b": {}}], "c~/d": {"e": []}});
        let mut all = Vec::new();
        pointers(&document, "", &mut all);
        assert_eq!(
            all,
            ["", "/a", "/a/0", "/a/1", "/a/2", "/a/3", "/a/4", "/a/4/b", "/c~0~1d", "/c~0~1d/e"]
        );
        let at = |pointer: &str, mutation| mutated(&document, pointer, mutation);
        assert_eq!(
            at("/a/0", Mutation::Null),
            Some(json!({"a": [null, "t", true, null, {"b": {}}], "c~/d": {"e": []}}))
        );
        assert_eq!(
            at("/c~0~1d", Mutation::Removed),
            Some(json!({"a": [1, "t", true, null, {"b": {}}]}))
        );
        assert_eq!(at("/a/0", Mutation::Removed), None);
        assert_eq!(at("", Mutation::Removed), None);
        assert_eq!(at("/a", Mutation::Emptied), Some(json!({"a": [], "c~/d": {"e": []}})));
        assert_eq!(
            at("/a/4", Mutation::Emptied),
            Some(json!({"a": [1, "t", true, null, {}], "c~/d": {"e": []}}))
        );
        assert_eq!(at("/a/4/b", Mutation::Emptied), None);
        let retypes: Vec<Value> =
            [json!(null), json!(true), json!(1.5), json!("s"), json!([1]), json!({"k": 1})]
                .iter()
                .map(retyped)
                .collect();
        assert_eq!(
            retypes,
            [json!("x"), json!("true"), json!("1.5"), json!(7), json!({}), json!([])]
        );
    }

    #[test]
    fn only_a_panic_or_an_unexpected_error_is_refused() {
        let decode = CorpusError::Decode { detail: "/search: .: invalid data".to_owned() };
        let not_found = CorpusError::NotFound { what: "/consult/acco".to_owned() };
        assert_eq!(refused("m", Ok(Err(decode))), None);
        assert_eq!(refused("m", Ok(Err(not_found))), None);
        assert_eq!(refused("m", Ok(Err(CorpusError::Timeout))), Some("m: Timeout".to_owned()));
        assert_eq!(refused("m", Err(Box::new("boom"))), Some("m: panic".to_owned()));
    }
}
