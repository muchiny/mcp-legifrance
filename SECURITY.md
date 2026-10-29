# Security policy / Politique de sécurité

English first, French below. / Version française plus bas.

## English

### Supported versions

Nothing is published yet (the workspace is at `0.3.0`, the version of the first release) and fixes land on `main`. From that release (`0.3.0`) and until 1.0, only the latest published minor version receives fixes.

### Reporting a vulnerability

Report privately through GitHub's "Private vulnerability reporting" (Security tab, "Report a vulnerability"). Do not open a public issue. There is no fallback e-mail address yet. Never attach PISTE credentials, tokens or a real audit log. We acknowledge a report within **5 business days** and assess it within **15 days**. Disclosure is coordinated: GitHub advisory, CVE if applicable, and the "Security" section of `CHANGELOG.md`.

### Scope

In scope: the repository, the `mcp-legifrance` binary (stdio MCP server and `check-config`), the artefacts built from it (release archives and installers, MCPB bundle, OCI image) and the repository workflows. Out of scope: the PISTE API and Légifrance (contact the DILA), MCP clients, the language model, and the third-party crates themselves (report those upstream; report here if the way we use one is the problem).

### Compromised PISTE credentials

Your keys are strictly reserved to your application (PISTE terms of use). If they leaked, appeared in a log, a shared configuration file or a repository, reset them on PISTE immediately, then report it to the DILA and the AIFE.

### Threat model and known limitations

Design reference: `docs/plan/06-qualite.md` §10 (threats MEN-01 to MEN-23).

**Trust boundary**

- stdio is not a sandbox. The MCP client and the server share one trust domain, so the server does not try to defend itself against its own client.
- The PISTE secret stays in the process environment: any process of the same account can read it. No command-line option accepts a secret (it would show in `ps`), `check-config` prints only "set (n characters)", and errors, logs and the audit log never contain it.
- Logs go to stderr only, never carry a query text, and stdout carries nothing but JSON-RPC.

**Returned texts are data, not instructions**

- Legal texts come from the upstream API and may contain anything, including sentences addressed to a language model. They are returned as data fields: HTML is reduced to text, control, invisible and bidirectional characters are removed, and notes are fixed templates. Tools are read-only and have no side effects.
- Residual risk: whether a model obeys an instruction hidden in a text is decided by the client and the model, not by this server. Treat any instruction found inside a returned text as untrusted.
- Tool descriptions are locked (`tests/tools.lock.json`), so that a new version cannot silently change what a client has approved.

**Audit log**

- One JSON line per tool call, in `LF_AUDIT_DIR`. It holds a hash of the arguments, never their values. `input_hash` is an unsalted SHA-256: a short or guessable input can be recovered by enumeration.
- The log has no integrity proof. A process of the same account can edit it, and nothing chains the lines. Symbolic links are refused for the log file, and rotation never deletes anything.
- On Unix the directory is created `0700` and files `0600`. A pre-existing directory or file that is more permissive is not modified: the server warns instead.
- On Windows the server sets no ACL. Files inherit the ACL of the user profile folder, which should be private, but this is **not verified** (V-165): check with `icacls` on your own machine.
- Search queries are sent to PISTE, which is unavoidable. They are never logged, never audited in clear, and there is no disk cache.

**Network**

- TLS goes through rustls with the platform certificate verifier. Invalid certificates are never accepted and redirects are not followed.
- By default only the official PISTE hosts are contacted. A custom `LF_PISTE_OAUTH_URL` or `LF_PISTE_BASE_URL` is accepted but warned about, and credentials are then sent to that host: set it only to a host you trust. There is no telemetry and no remote configuration.
- The default rate limit is 5 requests per second. Over-consumption can get your PISTE application suspended, which is not under our control.
- The PISTE sandbox has no service commitment, and its data are not identical to production.

**Supply chain**

- `Cargo.lock` is committed and every build uses `--locked`. `cargo deny` checks advisories, licences, bans and sources on each pull request and every night. A crate version must be at least 14 days old. GitHub Actions are pinned by commit SHA and the workflows are linted. The Rust toolchain and `rmcp` are pinned.
- Third-party licences are listed in `THIRD_PARTY_LICENSES.md`.
- Releases are set up to carry SHA-256 sums and GitHub build-provenance attestations. No SLSA level is claimed. Windows and macOS binaries and the MCPB bundle are **not code-signed**: your system may warn when you run them.

**HTTP transport (optional, not in the default binaries)**

stdio is the transport of the published binaries. If you build the optional HTTP transport (`serve-http`), these rules apply:

- It listens on `127.0.0.1` or `[::1]` only; a non-loopback address is refused. Binding to the loopback address is not an authentication boundary, so every request must carry `Authorization: Bearer <token>`. The token is read from the environment, never from a command-line option, and compared in constant time.
- This static token is a deliberate deviation from the OAuth 2.1 authorisation that the MCP specification recommends for HTTP. The OAuth "remote" profile is **not offered**: PISTE keys are reserved to your application (open question Q4). Do not expose this server on a network, and never share it between several people.
- The `Origin` header is validated (a bad one gets a 403, against DNS rebinding), request bodies are capped at 256 KiB, tool invocations are rate-limited, `GET` and `DELETE` answer 405, and no session is created.
- The inbound token is never forwarded to PISTE, logged or audited. The two trust relationships (client to server, server to PISTE) are never mixed.

### Hardening advice

- Put credentials in the MCP client configuration, with restrictive file permissions, never in a repository.
- Test against the PISTE sandbox before using production.
- Verify what you download: `sha256sum -c sha256.sum --ignore-missing` for archives, the `fileSha256` of an MCPB bundle, and `gh attestation verify <file> --repo muchiny/mcp-legifrance`.
- Keep the audit directory private, and purge old logs yourself: nothing deletes them.
- Never publish your keys.

## Français

### Versions prises en charge

Rien n'est encore publié (le workspace est en `0.3.0`, version de la première release) et les correctifs vont sur `main`. À partir de cette version (`0.3.0`) et jusqu'à la 1.0, seule la dernière version mineure publiée reçoit des correctifs.

### Signaler une vulnérabilité

Signalez en privé par le « Private vulnerability reporting » de GitHub (onglet Security, « Report a vulnerability »). N'ouvrez pas d'issue publique. Il n'existe pas encore d'adresse électronique de repli. Ne joignez jamais d'identifiants PISTE, de jeton ni de journal d'audit réel. Accusé de réception sous **5 jours ouvrés**, évaluation sous **15 jours**. La divulgation est coordonnée : avis GitHub, CVE si applicable, rubrique « Security » du `CHANGELOG.md`.

### Périmètre

Dans le périmètre : le dépôt, le binaire `mcp-legifrance` (serveur MCP stdio et `check-config`), les artefacts qui en sont tirés (archives et installeurs de release, bundle MCPB, image OCI) et les workflows du dépôt. Hors périmètre : l'API PISTE et Légifrance (contacter la DILA), les clients MCP, le modèle de langage, et les crates tierces elles-mêmes (à signaler chez leurs auteurs ; à signaler ici si c'est notre façon de les employer qui pose problème).

### Identifiants PISTE compromis

Vos clés sont strictement réservées à votre application (CGU de PISTE). Si elles ont fuité, ou sont apparues dans un journal, un fichier de configuration partagé ou un dépôt, réinitialisez-les tout de suite sur PISTE, puis signalez-le à la DILA et à l'AIFE.

### Modèle de menace et limites connues

Référence de conception : `docs/plan/06-qualite.md` §10 (menaces MEN-01 à MEN-23).

**Frontière de confiance**

- stdio n'est pas un bac à sable. Le client MCP et le serveur partagent un même domaine de confiance : le serveur ne cherche donc pas à se protéger de son propre client.
- Le secret PISTE reste dans l'environnement du processus : tout processus du même compte peut le lire. Aucune option de ligne de commande n'accepte un secret (il se lirait dans `ps`), `check-config` n'affiche que « set (n characters) », et ni les erreurs, ni les journaux, ni le journal d'audit ne le contiennent.
- Les journaux vont sur stderr seulement, ne portent jamais le texte d'une requête, et stdout ne porte que du JSON-RPC.

**Les textes renvoyés sont des données, pas des instructions**

- Les textes juridiques viennent de l'API amont et peuvent contenir n'importe quoi, y compris des phrases adressées à un modèle de langage. Ils sont rendus comme des champs de données : le HTML est ramené à du texte, les caractères de contrôle, invisibles et bidirectionnels sont retirés, et les notes sont des gabarits fixes. Les outils sont en lecture seule et n'ont aucun effet de bord.
- Risque résiduel : qu'un modèle obéisse ou non à une consigne cachée dans un texte, c'est le client et le modèle qui en décident, pas ce serveur. Toute consigne trouvée dans un texte renvoyé est à tenir pour non fiable.
- Les descriptions d'outils sont verrouillées (`tests/tools.lock.json`) : une nouvelle version ne peut pas changer en silence ce qu'un client a approuvé.

**Journal d'audit**

- Une ligne JSON par appel d'outil, dans `LF_AUDIT_DIR`. Elle contient une empreinte des arguments, jamais leurs valeurs. `input_hash` est un SHA-256 non salé : une entrée courte ou devinable se retrouve par énumération.
- Le journal n'a pas de preuve d'intégrité. Un processus du même compte peut le modifier, et rien n'enchaîne les lignes. Les liens symboliques sont refusés pour le fichier journal, et la rotation ne supprime jamais rien.
- Sous Unix, le répertoire est créé en `0700` et les fichiers en `0600`. Un répertoire ou un fichier préexistant plus permissif n'est pas modifié : le serveur avertit.
- Sous Windows, le serveur ne pose aucune ACL. Les fichiers héritent de l'ACL du dossier de profil, a priori privée, mais ce point n'est **pas vérifié** (V-165) : contrôlez-le avec `icacls` sur votre poste.
- Les requêtes de recherche partent vers PISTE, ce qui est inévitable. Elles ne sont jamais journalisées ni auditées en clair, et il n'y a aucun cache sur disque.

**Réseau**

- TLS passe par rustls avec le vérificateur de certificats de la plateforme. Un certificat invalide n'est jamais accepté et les redirections ne sont pas suivies.
- Par défaut, seuls les hôtes officiels de PISTE sont contactés. Un `LF_PISTE_OAUTH_URL` ou `LF_PISTE_BASE_URL` personnalisé est accepté mais signalé par un avertissement, et les identifiants partent alors vers cet hôte : ne le renseignez que pour un hôte de confiance. Il n'y a ni télémétrie ni configuration distante.
- Le débit par défaut est de 5 requêtes par seconde. Une surconsommation peut faire suspendre votre application PISTE, ce que nous ne maîtrisons pas.
- La sandbox PISTE n'a pas d'engagement de service, et ses données ne sont pas celles de la production.

**Chaîne d'approvisionnement**

- `Cargo.lock` est versionné et toute compilation emploie `--locked`. `cargo deny` contrôle les avis, les licences, les interdits et les sources à chaque PR et chaque nuit. Une version de crate doit avoir au moins 14 jours. Les actions GitHub sont épinglées par SHA de commit et les workflows sont analysés. La chaîne d'outils Rust et `rmcp` sont épinglés.
- Les licences tierces sont listées dans `THIRD_PARTY_LICENSES.md`.
- Les releases sont prévues avec des sommes SHA-256 et des attestations de provenance GitHub. Aucun niveau SLSA n'est revendiqué. Les binaires Windows et macOS et le bundle MCPB ne sont **pas signés** : votre système peut avertir au lancement.

**Transport HTTP (facultatif, absent des binaires par défaut)**

stdio est le transport des binaires publiés. Si vous compilez le transport HTTP facultatif (`serve-http`), ces règles s'appliquent :

- Il n'écoute que sur `127.0.0.1` ou `[::1]` ; une adresse hors boucle locale est refusée. L'adresse de boucle locale n'est pas une frontière d'authentification : chaque requête doit donc porter `Authorization: Bearer <jeton>`. Le jeton est lu dans l'environnement, jamais dans une option de ligne de commande, et comparé en temps constant.
- Ce jeton statique est un écart assumé à l'autorisation OAuth 2.1 que la spécification MCP recommande en HTTP. Le profil « distant » OAuth n'est **pas proposé** : les clés PISTE sont réservées à votre application (question ouverte Q4). N'exposez pas ce serveur sur un réseau et ne le partagez jamais entre plusieurs personnes.
- L'en-tête `Origin` est validé (un `Origin` invalide reçoit un 403, contre le rebinding DNS), les corps de requête sont plafonnés à 256 Kio, les appels d'outils sont limités en débit, `GET` et `DELETE` répondent 405, et aucune session n'est créée.
- Le jeton entrant n'est jamais transmis à PISTE, journalisé ni audité. Les deux relations de confiance (client vers serveur, serveur vers PISTE) ne sont jamais mélangées.

### Conseils de durcissement

- Placez les identifiants dans la configuration du client MCP, avec des droits de fichier restreints, jamais dans un dépôt.
- Faites vos essais sur la sandbox PISTE avant la production.
- Vérifiez ce que vous téléchargez : `sha256sum -c sha256.sum --ignore-missing` pour les archives, le `fileSha256` d'un bundle MCPB, et `gh attestation verify <fichier> --repo muchiny/mcp-legifrance`.
- Gardez le répertoire d'audit privé et purgez vous-même les anciens journaux : rien ne les supprime.
- Ne publiez jamais vos clés.
