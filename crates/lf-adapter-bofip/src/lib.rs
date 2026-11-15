//! Adapter BOFiP : la doctrine fiscale lue sur le portail ouvert de la DGFiP (ADR-0028,
//! ADR-0029).
//!
//! Seule implémentation du port [`TaxDoctrine`](lf_application::ports::TaxDoctrine) :
//! [`BofipDoctrine`]. Elle interroge le jeu `bofip-vigueur` de `data.economie.gouv.fr` (API
//! Opendatasoft « Explore v2.1 », sans compte ni clé, Licence Ouverte 2.0), par des `GET` sur une
//! seule adresse fixée dans le code.
//!
//! Ce que le crate garantit, quel que soit l'appelant :
//!
//! - **une requête fermée** : `https` seul, aucune redirection suivie, aucune variable ne change
//!   l'adresse ; les mots de l'appelant n'entrent dans la requête que comme un littéral échappé,
//!   écrit par une seule fonction qui ne reçoit que des valeurs typées par le domaine
//!   (`query`) ;
//! - **une réponse bornée et contrôlée** : corps plafonné après décompression, identifiants
//!   rendus validés et, en lecture, égaux à celui demandé, jamais plus de résultats que demandé
//!   (`mapping`) ;
//! - **un débit borné** : deux départs par seconde, deux lectures en vol, un budget par jour, un
//!   repos après un refus 429, aucun réessai, aucun cache (`pace`) ;
//! - **des erreurs muettes sur la requête** : aucune erreur ni aucun événement de journal ne
//!   porte l'adresse, les mots cherchés, un message du portail ou un extrait de corps : en `GET`,
//!   l'adresse contient les mots (`http`).

mod doctrine;
mod dto;
mod http;
mod mapping;
mod pace;
mod query;
#[cfg(test)]
mod testing;
mod time;

pub use doctrine::{BofipConfig, BofipDoctrine, RECORDS_URL};
