# Security policy / Politique de sécurité

English first, French below. / Version française plus bas.

## English

### Supported versions

Nothing is published yet (the workspace is at `0.0.0`). Until 1.0, only the latest published minor version receives fixes.

### Reporting a vulnerability

Report privately through GitHub's "Private vulnerability reporting" (Security tab, "Report a vulnerability"). Do not open a public issue. There is no fallback e-mail address yet. Never attach PISTE credentials, tokens or a real audit log. We acknowledge a report within **5 business days** and assess it within **15 days**. Disclosure is coordinated: GitHub advisory, CVE if applicable, and the "Security" section of `CHANGELOG.md`.

### Scope

Today: the repository, `lf-domain` and the workflows. The `mcp-legifrance` binary enters scope at milestone M4, then the MCPB bundle and the OCI image. Out of scope: the PISTE API and Légifrance (contact the DILA), MCP clients, and the language model.

### Compromised PISTE credentials

Reset them on PISTE immediately, then report to the DILA and the AIFE (PISTE terms of use).

### Threat model and known limitations

Planned design (`docs/plan/06-qualite.md` §10, applicable from M4):

- stdio is not a sandbox.
- The PISTE secret stays in the process environment.
- `input_hash` is an unsalted SHA-256.
- The audit log has no integrity proof.
- Returned texts are data, not instructions.
- The PISTE sandbox has no service commitment.

### Hardening advice

- Put credentials in the MCP client configuration, with restrictive file permissions.
- Test against the sandbox.
- Check the `fileSha256` of an MCPB bundle and the signature of binaries once they exist.
- Never publish your keys.

## Français

### Versions prises en charge

Rien n'est encore publié (le workspace est en `0.0.0`). Jusqu'à la 1.0, seule la dernière version mineure publiée reçoit des correctifs.

### Signaler une vulnérabilité

Signalez en privé par le « Private vulnerability reporting » de GitHub (onglet Security, « Report a vulnerability »). N'ouvrez pas d'issue publique. Il n'existe pas encore d'adresse électronique de repli. Ne joignez jamais d'identifiants PISTE, de jeton ni de journal d'audit réel. Accusé de réception sous **5 jours ouvrés**, évaluation sous **15 jours**. La divulgation est coordonnée : avis GitHub, CVE si applicable, rubrique « Security » du `CHANGELOG.md`.

### Périmètre

Aujourd'hui : le dépôt, `lf-domain` et les workflows. Le binaire `mcp-legifrance` entre dans le périmètre au jalon M4, puis le bundle MCPB et l'image OCI. Hors périmètre : l'API PISTE et Légifrance (contacter la DILA), les clients MCP et le modèle de langage.

### Identifiants PISTE compromis

Réinitialisez-les tout de suite sur PISTE, puis signalez-le à la DILA et à l'AIFE (CGU de PISTE).

### Modèle de menace et limites connues

Conception prévue (`docs/plan/06-qualite.md` §10, valable à partir de M4) :

- stdio n'est pas un bac à sable.
- Le secret PISTE reste dans l'environnement du processus.
- `input_hash` est un SHA-256 non salé.
- Le journal d'audit n'a pas de preuve d'intégrité.
- Les textes renvoyés sont des données, pas des instructions.
- La sandbox PISTE n'a pas d'engagement de service.

### Conseils de durcissement

- Placez les identifiants dans la configuration du client MCP, avec des droits de fichier restreints.
- Faites vos essais sur la sandbox.
- Vérifiez le `fileSha256` d'un bundle MCPB et la signature des binaires quand ils existeront.
- Ne publiez jamais vos clés.
