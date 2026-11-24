//! Adapter Service-Public : les fiches pratiques de Service-Public.gouv.fr (DILA), lues dans
//! l'archive du jour de chaque public, gardée en mémoire (ADR-0033).
//!
//! L'archive `vosdroits-latest.zip` d'un public est téléchargée au premier besoin, gardée en
//! mémoire et jamais écrite sur disque ; la recherche et la lecture se font dans cette copie, sans
//! requête : ni mot cherché ni numéro de fiche ne quitte la machine.
//!
//! Ce que le crate garantit, quel que soit l'appelant :
//!
//! - **une requête fermée** : deux adresses fixes, `https` seul, aucune redirection suivie,
//!   corps borné à 64 Mio, aucun réessai, repos après un 429, erreurs sans adresse (`client`) ;
//! - **une lecture d'archive bornée** : sous-ensemble du format servi seulement, répertoire
//!   central recoupé avec chaque en-tête local, entrées qui ne se chevauchent pas, décompression
//!   arrêtée au premier octet au-delà de la borne, somme CRC-32 vérifiée (`archive`) ;
//! - **un XML lu sans risque** : `DOCTYPE` et entités inconnues refusés, profondeur et nombre
//!   d'éléments bornés, aucune erreur ne cite le texte lu (`xml`) ;
//! - **une page traduite sans perte silencieuse** : numéro de la page recoupé avec celui demandé,
//!   éléments inconnus traversés et leur texte gardé, références de Légifrance seules (`page`) ;
//! - **une archive par public en mémoire** : un téléchargement à la fois, partagé ; attente bornée
//!   par l'échéance de l'appel ; revalidée au bout de 24 h, servie 48 h de plus après un échec ;
//!   index construit hors du fil de l'appel et servi entier (`store`).

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "lecteur d'archive lu par la source Service-Public (T-MS-25)")
)]
mod archive;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "client lu par la source Service-Public (T-MS-23)")
)]
mod client;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "traduction lue par la source Service-Public (T-MS-25)")
)]
mod page;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "magasin lu par la source Service-Public (T-MS-25)")
)]
mod store;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "lecteur XML lu par la source Service-Public (T-MS-25)")
)]
mod xml;
