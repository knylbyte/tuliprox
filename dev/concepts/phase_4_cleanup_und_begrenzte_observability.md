# Coding-Agent-Auftrag: Phase 4 – Cleanup und begrenzte Observability

Implementiere ausschließlich **Phase 4** der Tuliprox-Anpassung für HTTP-`Content-Encoding` bei HLS.

Die bindende technische Spezifikation ist:

```text
dev/concepts/hls-content-encoding/hls_content_encoding_policy_and_processing_concept.md
```

Maßgeblich sind insbesondere die DRY-Bereinigung alter Decoderpfade, die kontrollierte zlib-/Deflate-Helfermigration,
der `Transfer-Encoding`-Callsite-Audit, sichere redigierte Logs, optional feste Counter innerhalb der bestehenden
Metrikarchitektur, die vollständige Regressionstestmatrix sowie **Phase 4** im Abschnitt „Implementierungsphasen gemäß
`AGENTS.md`“.

## Verbindliche Vorbereitung

Führe vor jeder Codeänderung diese Schritte aus:

1. Lies die Root-`AGENTS.md` vollständig.
2. Suche nach weiteren anwendbaren `AGENTS.md` in und oberhalb aller betroffenen Verzeichnisse.
3. Lies das Content-Encoding-Konzept vollständig.
4. Lies die in der aktuellen Root-`AGENTS.md` als bindend bezeichnete HLS-Zielspezifikation. Zum Zeitpunkt der
   Prompt-Erstellung lautet der Pfad:

   ```text
   dev/concepts/hls-object-cache/xtream_hls_cache_proxy_zielkonzept_v19.md
   ```

   Ein aktuell in `AGENTS.md` genannter anderer Pfad hat Vorrang.
5. Prüfe, ob die Phasen 1 bis 3 vollständig vorhanden und getestet sind. Erstelle vor Änderungen eine Traceability-Liste
   der zentralen Decoder-, Request-, Cache-, Passthrough- und Legacy-Integrationspunkte.
6. Suche repositoryweit nach alter oder paralleler Content-Decoding-, `Content-Encoding`-, `Transfer-Encoding`-, gzip-,
   Deflate-, Brotli- und Zstandard-Logik.

Fehlen notwendige Ergebnisse aus früheren Phasen, implementiere sie nicht stillschweigend als Teil des Cleanups.
Dokumentiere konkrete Blocker. Fehlt eine bindende Spezifikation, darf ihr Inhalt nicht geraten werden.

## Arbeitsmodus

Arbeite zuerst im Plan-Modus. Der Plan muss enthalten:

1. zu ändernde und neu anzulegende Dateien,
2. Ergebnis des Duplikat- und Callsite-Audits,
3. exakten Phase-4-Scope,
4. Tests und Dokumentationsanpassungen,
5. Risiken und Annahmen,
6. ausdrücklich nicht umgesetzte allgemeine Observability- oder Proxy-Refactorings.

Beginne erst danach mit Änderungen. Phase 4 ist ein Cleanup- und Härtungs-Scope, keine Erlaubnis für neue fachliche
Features oder breite Architekturumbauten.

## Scope dieser Phase

Setze ausschließlich die folgenden Arbeitspakete um:

1. Entferne alte, nun redundante Content-Decoding-Implementierungen oder lasse bestehende öffentliche beziehungsweise
   weit verbreitete Helper schmal an den zentralen Content-Coding-Layer delegieren.
2. Stelle sicher, dass im Repository für denselben Zweck nur eine maßgebliche Decoderimplementierung existiert.
3. Führe den im Konzept beschriebenen präzisen zlib-Header-Helper ein beziehungsweise konsolidiere ihn:

   ```rust
   is_zlib_header
   ```

4. Migriere bestehende `is_deflate`-Callsites kontrolliert. Behalte einen delegierenden Kompatibilitätswrapper nur dann,
   wenn er für einen kleinen, reviewbaren Übergang notwendig ist und keine falsche Semantik fortschreibt.
5. Stelle sicher, dass `Content-Encoding: deflate` korrekt zwischen zlib-wrapped und Raw-Deflate unterscheidet.
6. Führe einen vollständigen Callsite-Audit für `Transfer-Encoding` durch. Entferne dessen Weitergabe überall dort, wo
   Tuliprox eine neue Axum-/HTTP-Response aufbaut. Ändere keine fachlich unabhängigen Transportpfade ohne Beleg.
7. Prüfe Preserve- und Identity-Pfade erneut auf repräsentationskonsistente Header. Erhalte die enge Allowlist-
   beziehungsweise Selector-Strategie; führe keine pauschale End-to-End-Headerkopie ein.
8. Ergänze sichere, redigierte Logs für abweichend codierte HLS-Origin-Antworten und Content-Decoding-Fehler gemäß
   Konzept. Logge keine signierten URLs, Query-Tokens, Cookies, Key-Bodies oder Manifestinhalte.
9. Ergänze optional ausschließlich feste `AtomicU64`-Counter, wenn dies ohne neue Metrikplattform in die bestehende
   Observability-Struktur passt. Zulässige Beispiele sind:
   - codierte Origin-Responses,
   - Content-Decoding-Fehler,
   - abgelehnte codierte Partial-Responses.
10. Führe keine gelabelte Legacy-/Shared-Metrikarchitektur ein.
11. Aktualisiere betroffene interne oder nutzerseitige Dokumentation nur, wenn das tatsächlich implementierte Verhalten
    dokumentationsrelevant ist. Führe keine neuen Konfigurationsfelder ein, sofern das Konzept dies nicht verlangt.
12. Ergänze oder konsolidiere Regressionstests, sodass die vollständige Zielmatrix des Konzepts über Legacy und Shared
    abgedeckt ist.
13. Entferne totes Code, ungenutzte Imports, überflüssige Exporte und unnötige Übergangsstrukturen, soweit dies
    unmittelbar durch die Content-Encoding-Änderungen entstanden ist.
14. Validere abschließend, dass Manifeste weiterhin Tower-komprimierbar und HLS-Binärresponses weiterhin gegen
    Doppelkompression geschützt sind.

## Explizit nicht enthalten

Implementiere in dieser Phase nicht:

- neue HLS-Funktionen,
- neue Cachevarianten pro Content-Coding,
- reqwest-Autodekompression,
- eine allgemeine Proxy-Headerplattform,
- pauschales Kopieren aller End-to-End-Header,
- eine neue Range-Policy,
- eine gelabelte Metrikarchitektur,
- großflächige Modulverschiebungen oder Namensbereinigungen außerhalb des Content-Encoding-Scope,
- neue Konfigurationsoptionen ohne ausdrückliche Konzeptanforderung,
- Änderungen an HLS-Inhaltsverschlüsselung oder Key-Material,
- Vollpufferung großer Streams.

Fachliche Lücken aus Phase 1 bis 3 sind als Blocker oder separater Folgeauftrag zu dokumentieren, nicht im Cleanup zu
verstecken.

## Akzeptanzkriterien

Die Phase gilt nur dann als abgeschlossen, wenn mindestens Folgendes nachweisbar ist:

- Es gibt genau eine maßgebliche Content-Coding-Decoderimplementierung.
- Alte Textdecoder delegieren an diese Implementierung oder wurden kontrolliert entfernt.
- zlib-wrapped und Raw-Deflate werden durch einen präzise benannten und getesteten Helper unterschieden.
- Falsch benannte oder duplizierte Deflate-Logik wurde bereinigt.
- `Transfer-Encoding` wird nicht in neu erzeugte Client-Responses kopiert.
- Preserve-Origin und HLS-Identity behalten repräsentationskonsistente Header.
- Keine breite Headerfreigabe exponiert Provider-Cookies, Authentifizierungs- oder proprietäre Header.
- Logs enthalten die nötigen technischen Felder, aber keine sensitiven Inhalte.
- Eine neue Metrikarchitektur wurde nicht eingeführt.
- Optionale Counter verwenden das bestehende feste Counter-Pattern und sind getestet.
- Die vollständige Legacy-/Shared-Zielmatrix des Konzepts ist durch bestehende oder ergänzte Tests abgedeckt.
- Es gibt keine neue Doppelkompression von HLS-Binärressourcen.
- Neu erzeugte Manifeste bleiben clientseitig komprimierbar.
- Workspace-Formatierung, Clippy und Tests sind erfolgreich oder transparent als Blocker dokumentiert.

## Tests und Audit-Nachweise

Ergänze beziehungsweise konsolidiere mindestens folgende Prüfungen:

- alle unterstützten deklarierte Codierungen,
- Raw-Deflate versus zlib-wrapped Deflate,
- mehrere Codierungen und mehrere Header,
- Manifest-Magic nur im HLS-Manifestmodus,
- finales Request-Enforcement über Merge, Retry, Provider-Wechsel und Redirect,
- Shared Manifest, Cache, Repair und Transient,
- Legacy Live-HLS und HLS-Catchup,
- initial/deferred/reconnect Modus-Propagation,
- codiertes `206`,
- Decoderfehler versus Cache-/Storage-Fehler,
- Preserve-Origin mit erhaltenem `Content-Encoding`,
- keine `Transfer-Encoding`-Weitergabe,
- keine Tower-Doppelkompression,
- redigierte Logging-Ausgaben soweit praktikabel testbar,
- feste Counter, falls eingeführt.

Dokumentiere im Abschlussbericht die Suchmuster oder Werkzeuge des Duplikat- und Header-Audits, beispielsweise
repositoryweite Suchen nach:

```text
Content-Encoding
Transfer-Encoding
GzipDecoder
ZlibDecoder
DeflateDecoder
BrotliDecoder
ZstdDecoder
is_deflate
is_zlib_header
```

## Codequalität und DRY

- Entferne Duplikate, ohne neue Abstraktionsschichten zu erzeugen.
- Halte öffentliche API eng; bevorzuge crate-interne Module und bestehende Exporte.
- Bewahre bestehende Tuliprox-Strukturen und Callsite-Verantwortlichkeiten.
- Keine positionalen `bool`-Parameter für semantische Modi.
- Keine pauschale Headerkopie.
- Keine neue Metrikplattform.
- Keine Helper ohne klare, wiederverwendbare Invariante.
- Aktualisiere Kommentare und Docstrings, wenn sie nach der Konsolidierung falsch oder missverständlich sind.

## Validierung

Führe zuerst fokussierte Regressionstests und anschließend die vollständigen Repository-Prüfungen aus. Erwartet werden
insbesondere:

```text
cargo +stable test -p tuliprox content_coding
cargo +stable test -p tuliprox hls
cargo +stable clippy -p tuliprox -- -D warnings
make fmt-check
make lint
make test
make markdown-lint
```

Wenn keine Markdown-Datei geändert wurde, dokumentiere, ob `make markdown-lint` trotzdem ausgeführt wurde oder warum es
für den Codechange nicht erforderlich war.

Melde keinen Prüfschritt als erfolgreich, wenn er nicht ausgeführt wurde oder fehlschlug. Dokumentiere exakten Befehl,
Ursache und Ersatzprüfung.

## Abschlussprüfung und Bericht

Validiere die fertige Phase erneut gegen:

1. alle anwendbaren `AGENTS.md`,
2. Phase 4 und die vollständige Zielmatrix des Content-Encoding-Konzepts,
3. die laut Root-`AGENTS.md` bindende allgemeine HLS-Zielspezifikation,
4. das Ergebnis des Decoderduplikat- und `Transfer-Encoding`-Audits,
5. die Trennung zwischen Manifest-Clientkompression und unkomprimierten HLS-Binärresponses.

Berichte zusätzlich kurz:

- welche alten Decoderpfade entfernt oder delegierend konsolidiert wurden,
- welche `Transfer-Encoding`-Callsites geprüft und geändert wurden,
- welche Logs und festen Counter ergänzt wurden,
- welche vollständigen Regressionen ausgeführt wurden,
- welche verbleibenden Punkte ausdrücklich außerhalb dieses Scopes liegen.

Der Abschlussbericht muss exakt mit folgendem Block enden:

```text
Changed files:
New files:
Acceptance criteria satisfied:
Tests / validation performed:
Open points / blockers:
```
