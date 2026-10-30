//! Index tantivy des documents LEGI : l'écriture ([`IndexStore`]) et la lecture ([`Engine`]).
//!
//! # Schéma
//!
//! Un document tantivy par **chemin** de membre d'archive (la clé de [`DocStore`], rapport 10
//! §10.1), jamais par identifiant : un même identifiant peut avoir deux copies, et la copie à lire
//! se choisit à la lecture par [`preferred_copy`].
//!
//! | Champ | Type | Rôle |
//! |---|---|---|
//! | `key` | texte brut, stocké | chemin sans `.xml` ; cible des suppressions de fichier |
//! | `text_dir` | texte brut | dossier du texte ; cible des suppressions de dossier |
//! | `id` | texte brut | `LEGIARTI…`, `LEGISCTA…`, `LEGITEXT…` |
//! | `dir_cid` | texte brut | identifiant du dossier de texte (`JORFTEXT…` ou `LEGITEXT…`) |
//! | `kind` | texte brut | `article`, `section`, `struct`, `version` |
//! | `tree` | texte brut | `code` ou `tnc` |
//! | `num` | texte brut | numéro d'article ou de texte, replié par [`fold_num`] |
//! | `nor` | texte brut | NOR du texte, en majuscules |
//! | `nature` | texte brut | nature (`CODE`, `LOI`…), en majuscules |
//! | `debut`, `fin` | entier rapide | fenêtre de vigueur en `AAAAMMJJ` ; absent = 0 et 99 999 999 |
//! | `title`, `body` | texte analysé | titres et corps, par [`french_analyzer`] |
//! | `payload` | octets stockés | le `Document` du module `model`, voir le module `codec` |
//!
//! La règle de vigueur est `debut <= D < fin` (rapport 10 §10.6) : `2999-01-01` est une fin
//! ouverte, `2222-02-22` une date différée indéterminée, un article `ENTIEREMENT_MODIF`
//! (`debut` = `fin` = `2999-01-01`) n'est jamais en vigueur, et un mort-né (fin avant début)
//! n'a pas de date possible.
//!
//! # Analyse du français
//!
//! [`french_analyzer`] découpe sur les non-alphanumériques, passe en minuscules, **replie les
//! accents** (`é` → `e`, `œ` → `oe`), retire les mots vides puis racinise avec le Snowball
//! français. Le repli précède la racinisation : `règlement` et `reglement` arrivent identiques au
//! racinisateur, donc se retrouvent l'un l'autre.
//!
//! # Écriture
//!
//! [`IndexStore`] implémente [`DocStore`] : l'ingestion y range les documents, et
//! [`DocStore::commit`] les rend durables. Les lectures ne voient que l'état du dernier commit.
//! Il n'y a qu'un écrivain à la fois (verrou tantivy du dossier).

use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    ops::Bound,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use lf_application::ports::CorpusError;
use lf_domain::{date::CivilDate, search::MatchMode, text::LongText};
use tantivy::{
    Index, IndexReader, IndexWriter, ReloadPolicy, Searcher, TantivyDocument, Term,
    collector::{Count, TopDocs},
    directory::{Directory, MmapDirectory, RamDirectory},
    query::{BooleanQuery, Occur, PhraseQuery, Query, RangeQuery, TermQuery},
    schema::{
        BytesOptions, Field, IndexRecordOption, NumericOptions, STORED, STRING, Schema,
        TextFieldIndexing, TextOptions, Value,
    },
    tokenizer::{
        AsciiFoldingFilter, Language, LowerCaser, RemoveLongFilter, SimpleTokenizer, Stemmer,
        StopWordFilter, TextAnalyzer,
    },
};
use tracing::warn;

use crate::{
    codec,
    error::StoreError,
    model::Document,
    path::{DocKind, Tree},
    store::{DocStore, StoredDoc, preferred_copy},
};

/// Nom de l'analyseur français dans le gestionnaire de tokenizers de l'index.
pub const ANALYZER: &str = "fr_legi";

/// Mémoire de l'écrivain tantivy (un seul fil d'indexation : le global fait 5,25 millions de
/// membres, et l'ingestion lit déjà en flux).
const WRITER_BUDGET_BYTES: usize = 64 * 1024 * 1024;

/// Longueur d'un jeton, en octets, à partir de laquelle ce n'est plus un mot : un jeton de cette
/// longueur ou plus est retiré.
const MAX_TOKEN_BYTES: usize = 64;

/// Fin ouverte d'une fenêtre de vigueur absente.
const OPEN_END: u64 = 99_999_999;

/// Mots vides français, **déjà repliés** (le filtre passe après le repli des accents) : articles,
/// pronoms, prépositions et auxiliaires courants. Les mots de la loi (`article`, `code`, `loi`,
/// `sous`…) n'y figurent pas.
const STOP_WORDS: &[&str] = &[
    "a", "au", "aux", "avec", "c", "ce", "ces", "d", "dans", "de", "des", "du", "elle", "elles",
    "en", "est", "et", "eux", "il", "ils", "j", "je", "l", "la", "le", "les", "leur", "leurs",
    "lui", "m", "ma", "mais", "me", "meme", "mes", "moi", "mon", "n", "ne", "nos", "notre", "nous",
    "on", "ou", "par", "pas", "pour", "qu", "que", "qui", "s", "sa", "se", "ses", "son", "sont",
    "sur", "t", "ta", "te", "tes", "toi", "ton", "tu", "un", "une", "vos", "votre", "vous", "y",
];

/// L'analyseur des champs `title` et `body`, et des requêtes qui les interrogent.
pub fn french_analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(MAX_TOKEN_BYTES))
        .filter(LowerCaser)
        .filter(AsciiFoldingFilter)
        .filter(StopWordFilter::remove(STOP_WORDS.iter().map(|word| (*word).to_owned())))
        .filter(Stemmer::new(Language::French))
        .build()
}

/// Numéro replié pour la comparaison : lettres et chiffres en majuscules, tirets gardés, tout le
/// reste (espaces, points, `°`) retiré. `L. 36-11` et `L36-11` se rejoignent.
pub fn fold_num(raw: &str) -> String {
    raw.chars().filter(|c| c.is_alphanumeric() || *c == '-').flat_map(char::to_uppercase).collect()
}

/// Texte brut d'un fragment HTML (celui de l'API : paragraphes, entités décodées). Un fragment
/// démesuré (plus de 16 millions de caractères) n'est pas indexé : mieux vaut le trouver par son
/// identifiant que pas du tout.
pub(crate) fn plain(html: &str) -> String {
    LongText::from_html(html).map(|text| text.as_str().to_owned()).unwrap_or_default()
}

/// Date en `AAAAMMJJ`, la valeur de repli quand elle manque.
fn date_key(date: Option<CivilDate>, missing: u64) -> u64 {
    date.map_or(missing, |date| {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "année, mois et jour d'une date civile : le résultat tient en 8 chiffres"
        )]
        let key =
            u64::from(date.year()) * 10_000 + u64::from(date.month()) * 100 + u64::from(date.day());
        key
    })
}

/// Les valeurs de `kind`.
fn kind_name(kind: DocKind) -> &'static str {
    match kind {
        DocKind::Article => "article",
        DocKind::Section => "section",
        DocKind::TextStruct => "struct",
        DocKind::TextVersion => "version",
    }
}

/// Les valeurs de `tree`.
fn tree_name(tree: Tree) -> &'static str {
    match tree {
        Tree::Code => "code",
        Tree::Tnc => "tnc",
    }
}

/// Les champs du schéma.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fields {
    key: Field,
    text_dir: Field,
    id: Field,
    dir_cid: Field,
    kind: Field,
    tree: Field,
    num: Field,
    nor: Field,
    nature: Field,
    debut: Field,
    fin: Field,
    title: Field,
    body: Field,
    payload: Field,
}

fn build_schema() -> (Schema, Fields) {
    let mut builder = Schema::builder();
    let analyzed = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(ANALYZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    );
    let date = NumericOptions::default().set_indexed().set_fast();
    let fields = Fields {
        key: builder.add_text_field("key", STRING | STORED),
        text_dir: builder.add_text_field("text_dir", STRING),
        id: builder.add_text_field("id", STRING),
        dir_cid: builder.add_text_field("dir_cid", STRING),
        kind: builder.add_text_field("kind", STRING),
        tree: builder.add_text_field("tree", STRING),
        num: builder.add_text_field("num", STRING),
        nor: builder.add_text_field("nor", STRING),
        nature: builder.add_text_field("nature", STRING),
        debut: builder.add_u64_field("debut", date.clone()),
        fin: builder.add_u64_field("fin", date),
        title: builder.add_text_field("title", analyzed.clone()),
        body: builder.add_text_field("body", analyzed),
        payload: builder.add_bytes_field("payload", BytesOptions::default().set_stored()),
    };
    (builder.build(), fields)
}

/// Ce que l'index retient d'un document en plus de sa charge utile.
#[derive(Debug, Default)]
struct Facts {
    num: Option<String>,
    nor: Option<String>,
    nature: Option<String>,
    debut: u64,
    fin: u64,
    title: String,
    body: String,
}

fn upper(text: Option<&String>) -> Option<String> {
    text.map(|text| text.trim().to_uppercase()).filter(|text| !text.is_empty())
}

fn facts(document: &Document) -> Facts {
    let window = |debut, fin| (date_key(debut, 0), date_key(fin, OPEN_END));
    match document {
        Document::Article(article) => {
            let (debut, fin) = window(article.date_debut, article.date_fin);
            let mut title = article.context.title.clone().unwrap_or_default();
            for section in &article.context.sections {
                title.push(' ');
                title.push_str(&section.title);
            }
            Facts {
                num: article.num.as_deref().map(fold_num).filter(|num| !num.is_empty()),
                nature: upper(article.context.nature.as_ref()),
                debut,
                fin,
                title: plain(&title),
                body: plain(&article.content),
                ..Facts::default()
            }
        }
        Document::Section(section) => Facts {
            debut: 0,
            fin: OPEN_END,
            title: plain(section.title.as_deref().unwrap_or_default()),
            ..Facts::default()
        },
        Document::TextStruct(text) => Facts {
            num: text.chronicle.num.as_deref().map(fold_num).filter(|num| !num.is_empty()),
            nor: upper(text.chronicle.nor.as_ref()),
            nature: upper(text.nature.as_ref()),
            debut: 0,
            fin: OPEN_END,
            ..Facts::default()
        },
        Document::TextVersion(text) => {
            let (debut, fin) = window(text.date_debut, text.date_fin);
            let title = [text.title.as_deref(), text.title_full.as_deref()].into_iter().flatten();
            Facts {
                num: text.chronicle.num.as_deref().map(fold_num).filter(|num| !num.is_empty()),
                nor: upper(text.chronicle.nor.as_ref()),
                nature: upper(text.nature.as_ref()),
                debut,
                fin,
                title: plain(&title.collect::<Vec<_>>().join(" ")),
                ..Facts::default()
            }
        }
    }
}

fn tantivy_document(fields: &Fields, doc: &StoredDoc) -> TantivyDocument {
    let facts = facts(&doc.document);
    let mut out = TantivyDocument::default();
    out.add_text(fields.key, &doc.path.key);
    out.add_text(fields.text_dir, &doc.path.text_dir);
    out.add_text(fields.id, &doc.path.id);
    out.add_text(fields.dir_cid, &doc.path.cid);
    out.add_text(fields.kind, kind_name(doc.path.kind));
    out.add_text(fields.tree, tree_name(doc.path.tree));
    for (field, value) in
        [(fields.num, &facts.num), (fields.nor, &facts.nor), (fields.nature, &facts.nature)]
    {
        if let Some(value) = value {
            out.add_text(field, value);
        }
    }
    out.add_u64(fields.debut, facts.debut);
    out.add_u64(fields.fin, facts.fin);
    out.add_text(fields.title, &facts.title);
    out.add_text(fields.body, &facts.body);
    out.add_bytes(fields.payload, &codec::encode(doc));
    out
}

fn store_error(action: &str, error: &impl Display) -> StoreError {
    StoreError::new(format!("cannot {action}: {error}"))
}

/// Le texte d'une erreur d'ouverture de l'index, pour les chaînes de `and_then` de l'ouverture.
fn text(error: impl Display) -> String {
    format!("cannot open the index: {error}")
}

/// Point d'injection de panne : le n-ième passage armé échoue à la place de l'appel à tantivy
/// qu'il garde. Les tests l'arment tour à tour sur chaque passage d'une opération pour mener
/// chaque chemin d'erreur ; en production il n'est jamais armé et ne coûte qu'une lecture
/// atomique.
#[derive(Debug, Default)]
struct Fault {
    countdown: AtomicU64,
}

impl Fault {
    /// Rend `result`, ou une panne si c'est le passage armé.
    fn pass<T>(&self, result: tantivy::Result<T>) -> tantivy::Result<T> {
        match self.countdown.load(Ordering::SeqCst) {
            0 => result,
            1 => {
                self.countdown.store(0, Ordering::SeqCst);
                Err(tantivy::TantivyError::InternalError("injected fault".to_owned()))
            }
            left => {
                self.countdown.store(left.saturating_sub(1), Ordering::SeqCst);
                result
            }
        }
    }

    /// Le `passes`-ième passage à venir échouera (1 : le prochain).
    #[cfg(test)]
    fn arm(&self, passes: u64) {
        self.countdown.store(passes, Ordering::SeqCst);
    }

    /// Reste-t-il une panne armée qui n'a pas eu lieu ?
    #[cfg(test)]
    fn armed(&self) -> bool {
        self.countdown.load(Ordering::SeqCst) != 0
    }
}

/// Une lecture de l'index a échoué : l'opérateur lit la cause dans les journaux, le modèle un
/// texte fixe (jamais un chemin du disque).
pub(crate) fn read_failure(error: &impl Display) -> CorpusError {
    warn!(error = %error, "the local DILA index could not be read");
    CorpusError::Config {
        kind: lf_application::ports::ConfigKind::Settings,
        detail: "the local DILA index could not be read; rebuild it from the dumps".to_owned(),
    }
}

/// Un enregistrement de l'index qui ne se décode plus (index d'une autre version du crate).
pub(crate) fn unreadable_entry() -> CorpusError {
    warn!("an entry of the local DILA index cannot be decoded");
    CorpusError::Decode { detail: "unreadable entry in the local DILA index".to_owned() }
}

/// Le moteur : l'index ouvert et son lecteur. Partagé (`Arc`) entre les lectures et l'écrivain.
pub struct Engine {
    index: Index,
    reader: IndexReader,
    fields: Fields,
    fault: Fault,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}

impl Engine {
    /// Ouvre l'index du dossier `dir`, le crée s'il n'existe pas.
    ///
    /// # Errors
    ///
    /// [`StoreError`] si le dossier est inutilisable ou si l'index qu'il contient a un autre
    /// schéma (à reconstruire depuis les dumps).
    pub fn open(dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(dir)
            .map_err(text)
            .and_then(|()| MmapDirectory::open(dir).map_err(text))
            .and_then(Self::build)
            .map_err(StoreError::new)
    }

    /// Un index en mémoire, vide : tests et petits dumps.
    ///
    /// # Errors
    ///
    /// [`StoreError`] si le lecteur ne peut pas être créé.
    pub fn in_memory() -> Result<Self, StoreError> {
        Self::build(RamDirectory::create()).map_err(StoreError::new)
    }

    /// Ouvre ou crée l'index de `directory` (un schéma différent est refusé), son analyseur et
    /// son lecteur.
    fn build(directory: impl Into<Box<dyn Directory>>) -> Result<Self, String> {
        let (schema, fields) = build_schema();
        Index::builder().schema(schema).open_or_create(directory).map_err(text).and_then(|index| {
            index.tokenizers().register(ANALYZER, french_analyzer());
            let builder = index.reader_builder().reload_policy(ReloadPolicy::Manual);
            TryInto::<IndexReader>::try_into(builder).map_err(text).map(|reader| Self {
                index,
                reader,
                fields,
                fault: Fault::default(),
            })
        })
    }

    /// Une lecture de l'index ; son échec devient l'erreur de lecture du port.
    fn read<T>(&self, result: tantivy::Result<T>) -> Result<T, CorpusError> {
        self.fault.pass(result).map_err(|e| read_failure(&e))
    }

    /// Une écriture de l'index ; son échec devient une [`StoreError`] qui nomme `action`.
    fn write<T>(&self, action: &str, result: tantivy::Result<T>) -> Result<T, StoreError> {
        self.fault.pass(result).map_err(|e| store_error(action, &e))
    }

    /// Le `passes`-ième accès à tantivy à venir échouera (1 : le prochain) ; 0 désarme.
    #[cfg(test)]
    pub(crate) fn fail_nth_access(&self, passes: u64) {
        self.fault.arm(passes);
    }

    /// Une panne armée n'a pas encore eu lieu : l'opération a fini avant d'y arriver.
    #[cfg(test)]
    pub(crate) fn fault_pending(&self) -> bool {
        self.fault.armed()
    }

    /// Une vue cohérente : toutes ses lectures voient le même état de l'index.
    pub(crate) fn view(&self) -> View<'_> {
        View { engine: self, searcher: self.reader.searcher() }
    }

    /// Nombre de documents (chemins) visibles.
    pub fn len(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    /// Vrai si l'index est vide.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Jetons de `text` d'après [`french_analyzer`], avec leur position d'origine (les mots vides
/// retirés laissent un trou, que la recherche de phrase respecte).
pub fn tokens(text: &str) -> Vec<(usize, String)> {
    let mut analyzer = french_analyzer();
    let mut stream = analyzer.token_stream(text);
    let mut tokens = Vec::new();
    while stream.advance() {
        let token = stream.token();
        tokens.push((token.position, token.text.clone()));
    }
    tokens
}

/// Une vue de lecture : un moteur et un état figé de l'index.
pub(crate) struct View<'a> {
    engine: &'a Engine,
    searcher: Searcher,
}

/// Les documents d'une recherche, du mieux classé au moins bon.
#[derive(Debug)]
pub(crate) struct Ranked {
    /// Documents, au plus la borne demandée.
    pub(crate) docs: Vec<StoredDoc>,
}

fn term(field: Field, value: &str) -> Box<dyn Query> {
    Box::new(TermQuery::new(Term::from_field_text(field, value), IndexRecordOption::Basic))
}

fn all_of(clauses: Vec<Box<dyn Query>>) -> BooleanQuery {
    BooleanQuery::new(clauses.into_iter().map(|clause| (Occur::Must, clause)).collect())
}

impl View<'_> {
    fn fields(&self) -> &Fields {
        &self.engine.fields
    }

    /// Les documents de `query`, au plus `limit` (`limit` > 0), décodés.
    fn fetch(&self, query: &dyn Query, limit: usize) -> Result<Ranked, CorpusError> {
        let top = self
            .engine
            .read(self.searcher.search(query, &TopDocs::with_limit(limit).order_by_score()))?;
        let mut docs = Vec::with_capacity(top.len());
        for (_, address) in top {
            let stored: TantivyDocument = self.engine.read(self.searcher.doc(address))?;
            let key = stored.get_first(self.fields().key).and_then(|value| value.as_str());
            let payload =
                stored.get_first(self.fields().payload).and_then(|value| value.as_bytes());
            let doc = key.zip(payload).and_then(|(key, payload)| codec::decode(payload, key));
            docs.push(doc.ok_or_else(unreadable_entry)?);
        }
        Ok(Ranked { docs })
    }

    /// Fenêtre de vigueur : `debut <= at < fin`.
    fn in_force(&self, at: CivilDate) -> Vec<Box<dyn Query>> {
        let at = date_key(Some(at), 0);
        let fields = self.fields();
        vec![
            Box::new(RangeQuery::new(
                Bound::Unbounded,
                Bound::Included(Term::from_field_u64(fields.debut, at)),
            )),
            Box::new(RangeQuery::new(
                Bound::Excluded(Term::from_field_u64(fields.fin, at)),
                Bound::Unbounded,
            )),
        ]
    }

    /// La copie à lire du document `id` de nature `kind` ([`preferred_copy`]).
    pub(crate) fn by_id(&self, kind: DocKind, id: &str) -> Result<Option<StoredDoc>, CorpusError> {
        let fields = self.fields();
        let query = all_of(vec![term(fields.id, id), term(fields.kind, kind_name(kind))]);
        let found = self.fetch(&query, 8)?.docs;
        Ok(preferred_copy(&found).cloned())
    }

    /// La copie à lire de la version de texte du dossier `dir_cid` ; celle dont l'identifiant est
    /// le dossier lui-même (un code) l'emporte.
    pub(crate) fn text_of_dir(&self, dir_cid: &str) -> Result<Option<StoredDoc>, CorpusError> {
        let fields = self.fields();
        let query = all_of(vec![
            term(fields.dir_cid, dir_cid),
            term(fields.kind, kind_name(DocKind::TextVersion)),
        ]);
        let found = self.fetch(&query, 8)?.docs;
        let own: Vec<&StoredDoc> = found.iter().filter(|doc| doc.path.id == dir_cid).collect();
        let pool = if own.is_empty() { found.iter().collect() } else { own };
        Ok(preferred_copy(pool).cloned())
    }

    /// Les articles du dossier `dir_cid` dont le numéro replié est `num` et en vigueur à `at`,
    /// une copie par identifiant, le plus récent d'abord.
    pub(crate) fn articles_numbered(
        &self,
        dir_cid: &str,
        num: &str,
        at: CivilDate,
    ) -> Result<Vec<StoredDoc>, CorpusError> {
        let fields = self.fields();
        let mut clauses = vec![
            term(fields.dir_cid, dir_cid),
            term(fields.kind, kind_name(DocKind::Article)),
            term(fields.num, num),
        ];
        clauses.extend(self.in_force(at));
        let found = self.fetch(&all_of(clauses), 64)?.docs;
        let mut articles = one_copy_per_id(found);
        articles.sort_by_key(|doc| {
            let debut = doc.document.as_article().and_then(|article| article.date_debut);
            std::cmp::Reverse((debut, doc.path.id.clone()))
        });
        Ok(articles)
    }

    /// Les articles numérotés `num` d'une famille de textes, en vigueur à `at` si une date est
    /// donnée.
    pub(crate) fn articles_numbered_in(
        &self,
        tree: Tree,
        num: &str,
        at: Option<CivilDate>,
        limit: usize,
    ) -> Result<Ranked, CorpusError> {
        let fields = self.fields();
        let mut clauses = vec![
            term(fields.tree, tree_name(tree)),
            term(fields.kind, kind_name(DocKind::Article)),
            term(fields.num, num),
        ];
        if let Some(at) = at {
            clauses.extend(self.in_force(at));
        }
        self.fetch(&all_of(clauses), limit)
    }

    /// Les versions de texte dont le champ `by` (numéro ou NOR) vaut `value`, en vigueur à `at`
    /// si une date est donnée.
    pub(crate) fn texts_where(
        &self,
        by: TextKey,
        value: &str,
        at: Option<CivilDate>,
        limit: usize,
    ) -> Result<Ranked, CorpusError> {
        let fields = self.fields();
        let field = match by {
            TextKey::Number => fields.num,
            TextKey::Nor => fields.nor,
        };
        let mut clauses =
            vec![term(fields.kind, kind_name(DocKind::TextVersion)), term(field, value)];
        if let Some(at) = at {
            clauses.extend(self.in_force(at));
        }
        self.fetch(&all_of(clauses), limit)
    }

    /// Les codes (versions de texte de nature `CODE`), abrogés compris, une copie par identifiant.
    pub(crate) fn codes(&self) -> Result<Vec<StoredDoc>, CorpusError> {
        let fields = self.fields();
        let query = all_of(vec![
            term(fields.kind, kind_name(DocKind::TextVersion)),
            term(fields.nature, "CODE"),
        ]);
        Ok(one_copy_per_id(self.fetch(&query, 2_000)?.docs))
    }

    /// Recherche plein texte dans les articles et les titres de textes de la famille `tree`,
    /// en vigueur à `at` si une date est donnée. `None` si la requête ne contient aucun mot
    /// (que des mots vides, par exemple).
    pub(crate) fn search(
        &self,
        tree: Tree,
        text: &str,
        mode: MatchMode,
        at: Option<CivilDate>,
        limit: usize,
    ) -> Result<Option<Ranked>, CorpusError> {
        let Some(words) = self.words_query(text, mode) else {
            return Ok(None);
        };
        let fields = self.fields();
        let kinds = BooleanQuery::new(vec![
            (Occur::Should, term(fields.kind, kind_name(DocKind::Article))),
            (Occur::Should, term(fields.kind, kind_name(DocKind::TextVersion))),
        ]);
        let mut clauses: Vec<Box<dyn Query>> =
            vec![Box::new(kinds), term(fields.tree, tree_name(tree)), words];
        if let Some(at) = at {
            clauses.extend(self.in_force(at));
        }
        self.fetch(&all_of(clauses), limit).map(Some)
    }

    /// La requête des mots de `text` sur le titre et le corps.
    fn words_query(&self, text: &str, mode: MatchMode) -> Option<Box<dyn Query>> {
        let tokens = tokens(text);
        if tokens.is_empty() {
            return None;
        }
        let fields = self.fields();
        let on_both = |value: &str| -> Box<dyn Query> {
            Box::new(BooleanQuery::new(vec![
                (Occur::Should, term(fields.title, value)),
                (Occur::Should, term(fields.body, value)),
            ]))
        };
        let query: Box<dyn Query> = match mode {
            MatchMode::Any | MatchMode::All => {
                let occur = if mode == MatchMode::All { Occur::Must } else { Occur::Should };
                Box::new(BooleanQuery::new(
                    tokens.iter().map(|(_, word)| (occur, on_both(word))).collect(),
                ))
            }
            MatchMode::Exact => {
                let phrase = |field: Field| -> Box<dyn Query> {
                    match tokens.as_slice() {
                        [(_, word)] => term(field, word),
                        _ => Box::new(PhraseQuery::new_with_offset(
                            tokens
                                .iter()
                                .map(|(position, word)| {
                                    (*position, Term::from_field_text(field, word))
                                })
                                .collect(),
                        )),
                    }
                };
                Box::new(BooleanQuery::new(vec![
                    (Occur::Should, phrase(fields.title)),
                    (Occur::Should, phrase(fields.body)),
                ]))
            }
        };
        Some(query)
    }
}

/// Champ de texte interrogeable par `texts_where`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextKey {
    /// Numéro du texte (`NUM`).
    Number,
    /// NOR.
    Nor,
}

/// Une copie par identifiant ([`preferred_copy`]), dans l'ordre de première apparition.
pub(crate) fn one_copy_per_id(docs: Vec<StoredDoc>) -> Vec<StoredDoc> {
    let mut order: Vec<String> = Vec::new();
    let mut copies: HashMap<String, Vec<StoredDoc>> = HashMap::new();
    for doc in docs {
        let id = doc.path.id.clone();
        copies
            .entry(id.clone())
            .or_insert_with(|| {
                order.push(id);
                Vec::new()
            })
            .push(doc);
    }
    order
        .iter()
        .filter_map(|id| copies.remove(id))
        .filter_map(|group| preferred_copy(&group).cloned())
        .collect()
}

/// L'écrivain de l'index : le [`DocStore`] de l'ingestion.
///
/// Un seul à la fois (verrou tantivy). Les écritures ne sont visibles des lectures qu'après
/// [`DocStore::commit`] ; l'écrivain, lui, valide ce qu'il faut avant de compter ce qu'il retire,
/// pour que les nombres rendus à l'ingestion soient exacts. Un écrivain lâché sans commit annule
/// ses écritures : l'index garde son dernier état durable.
pub struct IndexStore {
    engine: Arc<Engine>,
    writer: IndexWriter,
    /// Des écritures attendent un commit.
    dirty: bool,
    /// Des ajouts ou une remise à zéro attendent : un comptage ne les verrait pas.
    needs_flush: bool,
    /// Clés retirées depuis le dernier commit (un second retrait ne compte pas).
    deleted_keys: HashSet<String>,
}

impl std::fmt::Debug for IndexStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexStore").field("dirty", &self.dirty).finish_non_exhaustive()
    }
}

impl IndexStore {
    /// Prend le verrou d'écriture de l'index de `engine`.
    ///
    /// # Errors
    ///
    /// [`StoreError`] si un autre écrivain tient déjà le verrou.
    pub fn new(engine: Arc<Engine>) -> Result<Self, StoreError> {
        let writer = engine
            .index
            .writer_with_num_threads(1, WRITER_BUDGET_BYTES)
            .map_err(|e| store_error("lock the index for writing", &e))?;
        Ok(Self { engine, writer, dirty: false, needs_flush: false, deleted_keys: HashSet::new() })
    }

    /// Nombre de documents dont le champ `field` vaut `value`, parmi ceux déjà validés.
    fn count(&self, field: Field, value: &str) -> Result<u64, StoreError> {
        let query = TermQuery::new(Term::from_field_text(field, value), IndexRecordOption::Basic);
        let count = self
            .engine
            .write("count documents", self.engine.reader.searcher().search(&query, &Count))?;
        Ok(u64::try_from(count).unwrap_or(u64::MAX))
    }
}

impl DocStore for IndexStore {
    fn reset(&mut self) -> Result<(), StoreError> {
        self.engine.write("empty the index", self.writer.delete_all_documents())?;
        self.deleted_keys.clear();
        self.dirty = true;
        self.needs_flush = true;
        Ok(())
    }

    fn put(&mut self, doc: StoredDoc) -> Result<(), StoreError> {
        let fields = self.engine.fields;
        self.writer.delete_term(Term::from_field_text(fields.key, &doc.path.key));
        let added = self.writer.add_document(tantivy_document(&fields, &doc));
        self.engine.write("add a document", added)?;
        self.deleted_keys.remove(&doc.path.key);
        self.dirty = true;
        self.needs_flush = true;
        Ok(())
    }

    fn remove_file(&mut self, key: &str) -> Result<bool, StoreError> {
        if self.deleted_keys.contains(key) {
            return Ok(false);
        }
        if self.needs_flush {
            self.commit()?;
        }
        let field = self.engine.fields.key;
        let existed = self.count(field, key)? > 0;
        if existed {
            self.writer.delete_term(Term::from_field_text(field, key));
            self.deleted_keys.insert(key.to_owned());
            self.dirty = true;
        }
        Ok(existed)
    }

    fn remove_text_dir(&mut self, text_dir: &str) -> Result<u64, StoreError> {
        if self.dirty {
            self.commit()?;
        }
        let field = self.engine.fields.text_dir;
        let removed = self.count(field, text_dir)?;
        if removed > 0 {
            self.writer.delete_term(Term::from_field_text(field, text_dir));
            self.dirty = true;
        }
        Ok(removed)
    }

    fn commit(&mut self) -> Result<(), StoreError> {
        if !self.dirty {
            return Ok(());
        }
        self.engine.write("commit the index", self.writer.commit())?;
        self.engine.write("reload the index", self.engine.reader.reload())?;
        self.dirty = false;
        self.needs_flush = false;
        self.deleted_keys.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Article, Chronicle, Section, TextContext, TextStruct, TextVersion},
        testing::{doc_key, every_fault, stored_doc, text_dir},
    };

    const CID: &str = "LEGITEXT000006069445";
    const ID: &str = "LEGIARTI000006284410";

    fn date(year: u16, month: u8, day: u8) -> Option<CivilDate> {
        CivilDate::new(year, month, day)
    }

    fn article(id: &str, num: &str, content: &str) -> Article {
        Article {
            id: id.into(),
            num: Some(num.into()),
            etat: Some("VIGUEUR".into()),
            kind: None,
            date_debut: date(2020, 1, 1),
            date_fin: date(2999, 1, 1),
            context: TextContext::default(),
            versions: Vec::new(),
            nota: String::new(),
            content: content.into(),
            links: Vec::new(),
        }
    }

    fn version(id: &str) -> TextVersion {
        TextVersion {
            id: id.into(),
            nature: Some("code".into()),
            chronicle: Chronicle {
                cid: Some(CID.into()),
                num: Some("2010 - 1".into()),
                nor: Some(" abcd1034567l ".into()),
                date_publi: None,
                date_texte: None,
                last_modified: None,
            },
            title: Some("Titre court".into()),
            title_full: Some("Titre long".into()),
            etat: None,
            date_debut: None,
            date_fin: None,
            authority: None,
            ministry: None,
            visas: String::new(),
            signatories: String::new(),
            tp: String::new(),
            nota: String::new(),
            abro: String::new(),
            rect: String::new(),
        }
    }

    fn put_article(store: &mut IndexStore, sub: &str, mtime: u64, content: &str) {
        let doc = Document::Article(article(ID, "L. 36-11", content));
        store.put(stored_doc(sub, CID, doc, mtime)).unwrap();
    }

    fn fresh() -> (Arc<Engine>, IndexStore) {
        let engine = Arc::new(Engine::in_memory().unwrap());
        let store = IndexStore::new(Arc::clone(&engine)).unwrap();
        (engine, store)
    }

    #[test]
    fn french_analyzer_folds_accents() {
        let words = |text: &str| -> Vec<String> {
            tokens(text).into_iter().map(|(_, word)| word).collect()
        };
        // l'accent et la casse ne changent rien, ni le pluriel (racinisation après le repli)
        assert_eq!(words("Règlement"), words("reglement"));
        assert_eq!(words("RÈGLEMENTS"), words("reglement"));
        assert_eq!(words("Été"), words("ete"));
        assert_eq!(words("Œuvre"), words("oeuvre"), "the ligature is spelled out");
        assert_eq!(words("garçon"), words("garcon"));
        assert_eq!(words("règlement").len(), 1);

        // les mots vides, accentués ou non, tombent ; les positions gardent le trou
        let spaced = tokens("le droit des étés, à même");
        let positions: Vec<usize> = spaced.iter().map(|(position, _)| *position).collect();
        assert_eq!(positions, [1, 3], "{spaced:?}");

        // un jeton démesuré n'est pas un mot
        assert_eq!(tokens(&"a".repeat(MAX_TOKEN_BYTES)).len(), 0);
        assert_eq!(tokens(&"a".repeat(MAX_TOKEN_BYTES - 1)).len(), 1);
    }

    #[test]
    fn numbers_fold_to_letters_digits_and_dashes() {
        assert_eq!(fold_num("L. 36-11"), "L36-11");
        assert_eq!(fold_num("l36-11"), "L36-11");
        assert_eq!(fold_num("R*123-4"), "R123-4");
        assert_eq!(fold_num("1°"), "1");
        assert_eq!(fold_num("é bis"), "ÉBIS");
        assert_eq!(fold_num("° ."), "");
    }

    #[test]
    fn dates_become_comparable_integers() {
        assert_eq!(date_key(date(2026, 10, 15), 0), 20_261_015);
        assert_eq!(date_key(date(2999, 1, 1), 0), 29_990_101);
        assert_eq!(date_key(None, 7), 7);
    }

    #[test]
    fn a_new_engine_is_empty_and_commits_show_up_in_reads_only_once_made() {
        let (engine, mut store) = fresh();
        assert!(engine.is_empty());
        put_article(&mut store, "code_en_vigueur", 1, "<p>texte</p>");
        assert_eq!(engine.len(), 0, "nothing is visible before the commit");
        assert!(format!("{store:?}").contains("dirty: true"));
        store.commit().unwrap();
        assert_eq!(engine.len(), 1);
        assert!(!engine.is_empty());
        store.commit().unwrap();
        assert_eq!(engine.len(), 1, "a commit with nothing to do is a no-op");
        assert!(format!("{engine:?}").starts_with("Engine"));
    }

    #[test]
    fn put_replaces_by_key_and_keeps_two_copies_under_two_directories() {
        let (engine, mut store) = fresh();
        put_article(&mut store, "code_en_vigueur", 1, "<p>premier</p>");
        put_article(&mut store, "code_en_vigueur", 2, "<p>second</p>");
        store.commit().unwrap();
        assert_eq!(engine.len(), 1, "same key, one document");
        put_article(&mut store, "code_non_vigueur", 3, "<p>copie</p>");
        store.commit().unwrap();
        assert_eq!(engine.len(), 2, "same id under another directory, two documents");

        let view = engine.view();
        let best = view.by_id(DocKind::Article, ID).unwrap().unwrap();
        assert_eq!(best.mtime, 3, "the newest copy is the one read");
        assert!(view.by_id(DocKind::Article, "LEGIARTI000000000000").unwrap().is_none());
        assert!(view.by_id(DocKind::Section, ID).unwrap().is_none(), "the kind is part of the key");
    }

    #[test]
    fn removing_a_file_counts_once_even_before_the_commit() {
        let (engine, mut store) = fresh();
        put_article(&mut store, "code_en_vigueur", 1, "<p>texte</p>");
        let key = doc_key("code_en_vigueur", CID, DocKind::Article, ID);
        // retirer ce que le dernier commit ne connaît pas encore force le commit
        assert!(store.remove_file(&key).unwrap());
        assert!(!store.remove_file(&key).unwrap(), "a second removal finds nothing");
        assert!(!store.remove_file("legi/global/absent").unwrap());
        store.commit().unwrap();
        assert_eq!(engine.len(), 0);

        // remettre une clé retirée la rend de nouveau retirable
        put_article(&mut store, "code_en_vigueur", 1, "<p>texte</p>");
        store.commit().unwrap();
        assert!(store.remove_file(&key).unwrap());
        put_article(&mut store, "code_en_vigueur", 2, "<p>revenu</p>");
        assert!(store.remove_file(&key).unwrap(), "put cleared the pending removal");
        store.commit().unwrap();
        assert_eq!(engine.len(), 0);
    }

    #[test]
    fn removing_a_text_dir_removes_its_whole_subtree() {
        let (engine, mut store) = fresh();
        put_article(&mut store, "code_en_vigueur", 1, "<p>a</p>");
        put_article(&mut store, "code_non_vigueur", 1, "<p>b</p>");
        let doc = Document::TextVersion(version(CID));
        store.put(stored_doc("code_en_vigueur", CID, doc, 1)).unwrap();
        let dir = text_dir("code_en_vigueur", CID);
        // le commit en attente est fait avant de compter
        assert_eq!(store.remove_text_dir(&dir).unwrap(), 2);
        assert_eq!(store.remove_text_dir(&dir).unwrap(), 0, "the removal is counted once");
        store.commit().unwrap();
        assert_eq!(engine.len(), 1, "the same text under non_vigueur is another folder");
    }

    #[test]
    fn reset_empties_the_index_and_a_dropped_writer_rolls_back() {
        let (engine, mut store) = fresh();
        put_article(&mut store, "code_en_vigueur", 1, "<p>a</p>");
        store.commit().unwrap();
        store.reset().unwrap();
        put_article(&mut store, "code_non_vigueur", 2, "<p>b</p>");
        assert_eq!(engine.len(), 1, "the reset is not durable before the commit");
        store.commit().unwrap();
        assert_eq!(engine.len(), 1);
        let view = engine.view();
        assert_eq!(view.by_id(DocKind::Article, ID).unwrap().unwrap().mtime, 2);
        drop(view);

        // un écrivain lâché sans commit annule ses écritures
        store.reset().unwrap();
        drop(store);
        assert_eq!(engine.len(), 1);
        let mut again = IndexStore::new(Arc::clone(&engine)).unwrap();
        again.commit().unwrap();
        assert_eq!(engine.len(), 1);
    }

    #[test]
    fn only_one_writer_holds_the_index() {
        let (engine, store) = fresh();
        let error = IndexStore::new(Arc::clone(&engine)).unwrap_err();
        assert!(error.to_string().contains("lock the index for writing"), "{error}");
        drop(store);
        assert!(IndexStore::new(engine).is_ok());
    }

    #[test]
    fn an_index_on_disk_survives_a_reopen_and_refuses_a_foreign_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("index");
        {
            let engine = Arc::new(Engine::open(&path).unwrap());
            let mut store = IndexStore::new(Arc::clone(&engine)).unwrap();
            put_article(&mut store, "code_en_vigueur", 1, "<p>durable</p>");
            store.commit().unwrap();
        }
        let engine = Engine::open(&path).unwrap();
        assert_eq!(engine.len(), 1);
        let found = engine.view().by_id(DocKind::Article, ID).unwrap().unwrap();
        assert_eq!(found.document.as_article().unwrap().content, "<p>durable</p>");

        let foreign = dir.path().join("foreign");
        std::fs::create_dir(&foreign).unwrap();
        let mut builder = Schema::builder();
        builder.add_text_field("other", STRING);
        Index::create_in_dir(&foreign, builder.build()).unwrap();
        let error = Engine::open(&foreign).unwrap_err();
        assert!(error.to_string().contains("open the index"), "{error}");

        let file = dir.path().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        let error = Engine::open(&file.join("below")).unwrap_err();
        assert!(error.to_string().contains("cannot open the index: "), "{error}");
    }

    #[test]
    fn an_entry_that_no_longer_decodes_is_a_decode_error() {
        let (engine, store) = fresh();
        drop(store);
        let fields = engine.fields;
        let key = doc_key("code_en_vigueur", CID, DocKind::Article, ID);
        let mut writer: IndexWriter = engine.index.writer_with_num_threads(1, 15_000_000).unwrap();
        let mut broken = TantivyDocument::default();
        broken.add_text(fields.key, &key);
        broken.add_text(fields.id, ID);
        broken.add_text(fields.kind, "article");
        broken.add_bytes(fields.payload, &[1, 2, 3]);
        writer.add_document(broken).unwrap();
        writer.commit().unwrap();
        engine.reader.reload().unwrap();
        let error = engine.view().by_id(DocKind::Article, ID).unwrap_err();
        assert_eq!(error, unreadable_entry());
    }

    #[test]
    fn the_text_of_a_folder_prefers_the_version_that_carries_the_folder_id() {
        let (engine, mut store) = fresh();
        let other = "LEGITEXT000006069446";
        for (id, mtime) in [(other, 9), (CID, 1)] {
            let doc = Document::TextVersion(version(id));
            store.put(stored_doc("code_en_vigueur", CID, doc, mtime)).unwrap();
        }
        store.commit().unwrap();
        let view = engine.view();
        assert_eq!(view.text_of_dir(CID).unwrap().unwrap().path.id, CID, "own id first");
        assert!(view.text_of_dir("JORFTEXT000000000000").unwrap().is_none());

        // sans version à l'identifiant du dossier, la plus récente des versions du dossier
        let (engine, mut store) = fresh();
        let doc = Document::TextVersion(version(other));
        store.put(stored_doc("code_en_vigueur", CID, doc, 1)).unwrap();
        store.commit().unwrap();
        assert_eq!(engine.view().text_of_dir(CID).unwrap().unwrap().path.id, other);
    }

    #[test]
    fn texts_and_codes_are_found_by_their_folded_keys() {
        let (engine, mut store) = fresh();
        store
            .put(stored_doc("code_en_vigueur", CID, Document::TextVersion(version(CID)), 1))
            .unwrap();
        store.commit().unwrap();
        let view = engine.view();
        let by_number = view.texts_where(TextKey::Number, "2010-1", None, 10).unwrap();
        assert_eq!(by_number.docs.len(), 1);
        let by_nor = view.texts_where(TextKey::Nor, "ABCD1034567L", None, 10).unwrap();
        assert_eq!(by_nor.docs.len(), 1);
        let lower = view.texts_where(TextKey::Nor, "abcd1034567l", None, 10).unwrap();
        assert_eq!(lower.docs.len(), 0, "NOR keys are upper case");
        let dated = view.texts_where(TextKey::Number, "2010-1", date(2026, 1, 1), 10).unwrap();
        assert_eq!(dated.docs.len(), 1, "a text without dates is in force at every date");
        assert_eq!(view.codes().unwrap().len(), 1, "nature is folded to upper case");
    }

    #[test]
    fn every_kind_of_document_is_indexed_with_its_facts() {
        let (engine, mut store) = fresh();
        let section = Section {
            id: "LEGISCTA000006108694".into(),
            title: Some("<b>Titre</b> premier".into()),
            context: TextContext::default(),
            structure: Vec::new(),
        };
        let text_struct = TextStruct {
            id: CID.into(),
            nature: Some("loi".into()),
            chronicle: Chronicle {
                cid: None,
                num: Some("80-217".into()),
                nor: Some("x".into()),
                date_publi: None,
                date_texte: None,
                last_modified: None,
            },
            versions: Vec::new(),
            structure: Vec::new(),
        };
        let mut titled = article(ID, "", "<p>corps</p>");
        titled.context.title = Some("Code".into());
        titled.context.nature = Some(" code ".into());
        titled.context.sections = vec![crate::model::SectionRef {
            id: "LEGISCTA000006108694".into(),
            title: "Livre".into(),
            date_debut: None,
            date_fin: None,
        }];
        for doc in [
            Document::Section(section),
            Document::TextStruct(text_struct),
            Document::Article(titled),
        ] {
            store.put(stored_doc("code_en_vigueur", CID, doc, 1)).unwrap();
        }
        store.commit().unwrap();
        assert_eq!(engine.len(), 3);
        let view = engine.view();
        // le titre de la section est du texte brut, repéré par mot
        let found = view.search(Tree::Code, "titre", MatchMode::All, None, 10).unwrap();
        assert!(found.is_none() || found.unwrap().docs.is_empty(), "sections are not searched");
        let found = view.search(Tree::Code, "livre", MatchMode::All, None, 10).unwrap().unwrap();
        assert_eq!(found.docs.len(), 1, "the section chain is part of the article title");
        assert_eq!(view.articles_numbered(CID, "", date(2026, 1, 1).unwrap()).unwrap().len(), 0);
    }

    #[test]
    fn one_copy_per_id_keeps_the_preferred_copy_in_first_seen_order() {
        let a = |sub: &str, mtime| {
            stored_doc(sub, CID, Document::Article(article(ID, "1", "<p>x</p>")), mtime)
        };
        let other = stored_doc(
            "code_en_vigueur",
            CID,
            Document::Article(article("LEGIARTI000006284411", "2", "<p>y</p>")),
            1,
        );
        let kept =
            one_copy_per_id(vec![a("code_en_vigueur", 1), other.clone(), a("code_non_vigueur", 5)]);
        assert_eq!(kept.len(), 2);
        assert_eq!((kept[0].path.id.as_str(), kept[0].mtime), (ID, 5));
        assert_eq!(kept[1], other);
    }

    #[test]
    fn articles_with_one_number_come_newest_first() {
        let (engine, mut store) = fresh();
        for (id, debut) in [
            ("LEGIARTI000000000001", date(2019, 1, 1)),
            ("LEGIARTI000000000002", date(2021, 1, 1)),
            ("LEGIARTI000000000003", date(2021, 1, 1)),
        ] {
            let mut versioned = article(id, "L. 36-11", "<p>x</p>");
            versioned.date_debut = debut;
            store.put(stored_doc("code_en_vigueur", CID, Document::Article(versioned), 1)).unwrap();
        }
        store.commit().unwrap();
        let view = engine.view();
        let found = view.articles_numbered(CID, "L36-11", date(2026, 1, 1).unwrap()).unwrap();
        let ids: Vec<&str> = found.iter().map(|doc| doc.path.id.as_str()).collect();
        assert_eq!(ids, ["LEGIARTI000000000003", "LEGIARTI000000000002", "LEGIARTI000000000001"]);

        // sans date, tous les articles de ce numéro dans la famille de textes
        let any = view.articles_numbered_in(Tree::Code, "L36-11", None, 10).unwrap();
        assert_eq!(any.docs.len(), 3);
        let dated = view.articles_numbered_in(Tree::Code, "L36-11", date(2020, 1, 1), 10).unwrap();
        assert_eq!(dated.docs.len(), 1, "only the first version is in force in 2020");
    }

    #[test]
    fn every_failed_read_comes_back_as_a_read_failure() {
        let (engine, mut store) = fresh();
        put_article(&mut store, "code_en_vigueur", 1, "<p>texte</p>");
        let text = Document::TextVersion(version(CID));
        store.put(stored_doc("code_en_vigueur", CID, text, 1)).unwrap();
        store.commit().unwrap();
        let view = engine.view();
        let at = date(2026, 1, 1).unwrap();
        let reads: Vec<Box<dyn Fn() -> Result<(), CorpusError> + '_>> = vec![
            Box::new(|| view.by_id(DocKind::Article, ID).map(|_| ())),
            Box::new(|| view.text_of_dir(CID).map(|_| ())),
            Box::new(|| view.articles_numbered(CID, "L36-11", at).map(|_| ())),
            Box::new(|| view.articles_numbered_in(Tree::Code, "L36-11", Some(at), 10).map(|_| ())),
            Box::new(|| view.texts_where(TextKey::Number, "2010-1", Some(at), 10).map(|_| ())),
            Box::new(|| view.codes().map(|_| ())),
            Box::new(|| view.search(Tree::Code, "texte", MatchMode::All, Some(at), 10).map(|_| ())),
        ];
        for read in &reads {
            // la recherche, puis la lecture de chacun des documents trouvés
            assert!(every_fault(&engine, read) >= 2);
        }
    }

    #[test]
    fn every_failed_write_comes_back_as_a_store_error() {
        let (engine, mut store) = fresh();
        let doc = || {
            let article = Document::Article(article(ID, "L. 36-11", "<p>texte</p>"));
            stored_doc("code_en_vigueur", CID, article, 1)
        };
        let key = doc_key("code_en_vigueur", CID, DocKind::Article, ID);
        let dir = text_dir("code_en_vigueur", CID);
        assert_eq!(every_fault(&engine, || store.reset()), 1);
        assert_eq!(every_fault(&engine, || store.put(doc())), 1);
        // l'ajout, le commit que force la lecture du dernier état (écriture puis rechargement),
        // puis le comptage
        let removal = || store.put(doc()).and_then(|()| store.remove_file(&key));
        assert_eq!(every_fault(&engine, removal), 4);
        let removal = || store.put(doc()).and_then(|()| store.remove_text_dir(&dir));
        assert_eq!(every_fault(&engine, removal), 4);
        assert_eq!(every_fault(&engine, || store.put(doc()).and_then(|()| store.commit())), 3);
    }

    #[test]
    fn a_read_failure_is_a_fixed_text_for_the_model() {
        let detail = "the local DILA index could not be read; rebuild it from the dumps";
        assert_eq!(
            read_failure(&"mmap failed at /secret/path"),
            CorpusError::Config {
                kind: lf_application::ports::ConfigKind::Settings,
                detail: detail.to_owned()
            }
        );
    }
}
