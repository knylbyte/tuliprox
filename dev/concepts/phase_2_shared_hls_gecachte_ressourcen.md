# Coding-Agent-Auftrag: Phase 2 – Shared-HLS gecachte Ressourcen

Implementiere ausschließlich **Phase 2** der Tuliprox-Anpassung für HTTP-`Content-Encoding` bei HLS.

Die bindende technische Spezifikation ist:

```text
dev/concepts/hls-content-encoding/hls_content_encoding_policy_and_processing_concept.md
```

Maßgeblich sind insbesondere die Identity-Invariante für Shared-HLS-Ressourcen, die Zuständigkeitsgrenze zur bestehenden
Range-Policy, die zentrale Integration in den Shared-Resource-Retry-Loop, die Fehlerklassifikation vor dem
Cache-/Storage-Mapping, die Repair- und Cache-Regeln, die Phase-2-Testmatrix sowie **Phase 2** im Abschnitt
„Implementierungsphasen gemäß `AGENTS.md`“.

## Verbindliche Vorbereitung

Führe vor jeder Codeänderung diese Schritte aus:

1. Lies die Root-`AGENTS.md` vollständig.
2. Suche nach weiteren anwendbaren `AGENTS.md` in und oberhalb aller betroffenen Verzeichnisse.
3. Lies das Content-Encoding-Konzept vollständig.
4. Lies die in der aktuellen Root-`AGENTS.md` als bindend bezeichnete HLS-Zielspezifikation. Zum Zeitpunkt der
   Prompt-Erstellung ist dies:

   ```text
   dev/concepts/hls-object-cache/xtream_hls_cache_proxy_zielkonzept_v19.md
   ```

   Ist dort inzwischen ein anderer Pfad angegeben, gilt der aktuelle Pfad.
5. Prüfe, ob Phase 1 vollständig und konsistent im aktuellen Branch vorhanden ist. Verifiziere insbesondere den
   zentralen Content-Coding-Layer, die `DecodedHttpResponse`-Entsprechung, `ContentCodingDetection::DeclaredOnly`, die
   Decoderfehler-Erkennung und das finale Outbound-Enforcement.

Fehlen notwendige Ergebnisse aus Phase 1, implementiere sie nicht stillschweigend in Phase 2. Dokumentiere die konkreten
fehlenden Voraussetzungen als Blocker oder beschränke dich auf nachweislich unabhängige Arbeiten.

Fehlt eine laut `AGENTS.md` bindende Spezifikation, darfst du ihren Inhalt nicht erraten.

## Arbeitsmodus

Arbeite zuerst im Plan-Modus. Der Plan muss enthalten:

1. zu ändernde und neu anzulegende Dateien,
2. die überprüften Phase-1-Voraussetzungen,
3. den exakten Phase-2-Scope,
4. Tests und lokale Testserver-Fixtures,
5. Risiken und Annahmen,
6. bewusst nicht umgesetzte Arbeiten aus Phase 3 und 4.

Beginne erst danach mit der Implementierung. Halte die Änderungen klein, reviewbar und jederzeit kompilierbar.

## Scope dieser Phase

Setze ausschließlich die folgenden Arbeitspakete um:

1. Integriere die zentrale Content-Coding-Dekompression einmalig in den bestehenden Shared-HLS-Resource-Retry-Loop.
2. Behalte die bestehende `HlsOriginByteRangeExpectation` beziehungsweise deren aktuelle Entsprechung als alleinige
   fachliche Quelle für zulässige Status- und Range-Semantik bei.
3. Wende die Reihenfolge aus dem Konzept an:

   ```text
   Origin-Response
       → bestehende Status-/Range-Prüfung
       → decode_response_to_identity(DeclaredOnly)
       → Commit-Closure
   ```

4. Führe keine Response-Policy pro Resource-Target ein. Alle Shared-HLS-Origin-Ressourcen besitzen dieselbe
   Identity-Invariante.
5. Stelle die Commit-Grenze des Retry-Loops so um, dass Segment-, Map- und cachefähige Transient-Consumer eine
   dekodierte Response beziehungsweise deren `DynReader` erhalten.
6. Sorge dafür, dass Segment Repair ausschließlich die dekodierte Identity-Repräsentation erhält.
   HTTP-`Content-Encoding` wird entfernt; HLS-Inhaltsverschlüsselung und Ciphertext bleiben unverändert.
7. Stelle sicher, dass Segment-, Map- und cachefähige Transient-Objekte ausschließlich als Identity-Repräsentation in
   den Shared Cache geschrieben werden.
8. Nutze das bestehende `max_object_bytes` des Cachewriters als autoritatives Limit auf der dekodierten Repräsentation.
   Führe kein redundantes generisches Decoded-Limit ein.
9. Erkenne streamzeitige Content-Decoding-Fehler vor der pauschalen Umwandlung in Cache-, Storage-, Disk-Full- oder
   Datei-I/O-Fehler.
10. Klassifiziere Decoderfehler als Origin-/Content-Fehler und integriere sie in die bestehende Retry-Disposition für
    vollständig konsumierte Cachewrites.
11. Lehne nicht-Identity-codiertes `206 Partial Content` nach der bestehenden Status-/Range-Prüfung als nicht sicher
    dekodierbar ab.
12. Sorge dafür, dass temporäre Cachedateien bei Decoderfehlern zuverlässig bereinigt werden.
13. Erhalte die vorhandenen Backpressure-, Deadline-, Retry-, Provider- und Cache-Lifecycle-Mechanismen.
14. Erzeuge sichere, redigierte Logs nur im Umfang, der für Phase 2 zur Diagnose erforderlich ist. Führe keine neue
    gelabelte Metrikarchitektur ein.

Passe konkrete Signaturen an den aktuellen Branch an, ohne die im Konzept festgelegten Zuständigkeitsgrenzen
aufzuweichen.

## Explizit nicht enthalten

Implementiere in dieser Phase nicht:

- Legacy-Provider-Streams,
- Shared-Key- oder nicht cachefähige Transient-Passthrough-Responses zum Client,
- `ProviderContentRepresentationMode`,
- globale Provider-Response-Header-Refactorings,
- pauschales Kopieren von End-to-End-Headern,
- neue Range-Typen oder eine zweite Range-Policy,
- Vollpufferung beliebig großer Segmente,
- allgemeine reqwest-Autodekompression,
- gelabelte Metriken,
- Cleanup alter Decoderduplikate, soweit sie nicht zwingend für die Phase-2-Kompilierung erforderlich sind,
- allgemeine Proxy-Framework-Umbauten.

Notwendige Folgearbeiten für direkte Streams oder Legacy gehören unter „Open points / blockers“.

## Akzeptanzkriterien

Die Phase gilt nur dann als abgeschlossen, wenn mindestens Folgendes nachweisbar ist:

- Der Shared-Resource-Retry-Loop dekodiert erfolgreiche Origin-Responses zentral und genau einmal nach Identity.
- Es existiert keine pro Resource-Target konfigurierbare oder duplizierte Decoder-Policy.
- Die bestehende Status-/Range-Prüfung bleibt vor der Dekompression maßgeblich.
- Ein codiertes `206` wird zuverlässig abgelehnt.
- gzip-, Deflate-, Brotli- und Zstandard-Segmente werden vor Repair und Cache dekodiert.
- Segment Repair erhält die korrekten Identity-Medien- oder Ciphertext-Bytes.
- Maps und cachefähige Transient-Objekte werden als Identity gespeichert.
- Cachedateien enthalten keine HTTP-Kompressionshülle.
- Shared-Cache-Ranges beziehen sich auf die dekodierte Datei.
- Das bestehende `max_object_bytes` begrenzt die dekodierte Repräsentation.
- Decoderfehler werden nicht als Disk-Full-, Storage- oder allgemeine Cache-I/O-Fehler klassifiziert.
- Decoderfehler können im vollständig konsumierenden Cachepfad gemäß bestehender Retry-Policy erneut versucht werden.
- Temporäre Cachedateien werden bei Fehlern entfernt.
- Kein direkter Client-Passthrough und kein Legacy-Pfad wurde in den Scope gezogen.

## Tests

Implementiere die Phase-2-relevanten Tests aus dem Konzept. Dazu gehören mindestens:

- gzip-, Brotli- und Zstandard-Segment vor Repair und Cache,
- Raw-Deflate, sofern der zentrale Decoder dies unterstützt und der Testpfad sinnvoll abdeckbar ist,
- Cachedatei enthält erwartete Medienbytes statt Kompressions-Magic,
- fMP4-Map wird als Identity gecacht,
- cachefähige Transient-Ressource wird als Identity gecacht,
- Cache-Range bezieht sich auf die dekodierte Datei,
- codiertes `206` wird abgelehnt,
- bestehende `HlsOriginByteRangeExpectation` bleibt maßgeblich,
- Decoderfehler wird als Content-/Origin-Fehler klassifiziert,
- Decoderfehler wird nicht als Disk-Full/Storage/Cache-I/O gemeldet,
- temporäre Cachedatei wird nach abgebrochener oder korrupter komprimierter Response entfernt,
- bestehende Retry-Policy greift bei vollständig konsumierten Decoderfehlern,
- HLS-Inhaltsverschlüsselung bleibt bytegenau unverändert.

Verwende für Netzwerkverhalten ausschließlich lokale Testserver nach bestehenden Repository-Patterns.

## Codequalität und DRY

- Verwende ausschließlich den in Phase 1 eingeführten zentralen Decoder.
- Integriere ihn an genau einem zentralen Shared-Resource-Boundary.
- Führe keine `response_policy` im `HlsOriginResourceFetchTarget` ein.
- Führe keine zweite Range-Policy oder Partial-Content-Abstraktion ein.
- Lasse Cachelimits beim Consumer und nicht in einer neuen generischen Decoder-Policy.
- Erhalte bestehende Backpressure und streamendes `AsyncRead`.
- Vermeide Vollpufferung großer Medienobjekte.
- Halte Content-Decoding-Fehler von Storagefehlern typisiert unterscheidbar.
- Bevorzuge private Module und schmale Exporte.

## Validierung

Führe zunächst fokussierte Tests und anschließend die Gesamtprüfungen aus. Erwartet werden insbesondere:

```text
cargo +stable test -p tuliprox hls_cache
cargo +stable test -p tuliprox hls
cargo +stable clippy -p tuliprox -- -D warnings
make fmt-check
make lint
make test
```

Falls Markdown-Dokumente geändert werden:

```text
make markdown-lint
```

Dokumentiere jeden nicht ausführbaren oder fehlgeschlagenen Befehl mit exaktem Kommando und Ursache.

## Abschlussprüfung und Bericht

Validiere die fertige Phase erneut gegen:

1. alle anwendbaren `AGENTS.md`,
2. Phase 2 des Content-Encoding-Konzepts,
3. die bestehende HLS-Range- und Cache-Architektur,
4. die laut Root-`AGENTS.md` bindende allgemeine HLS-Zielspezifikation.

Berichte zusätzlich kurz:

- an welcher zentralen Stelle die Shared-Response dekodiert wird,
- wie Decoderfehler vor dem Cache-/Storage-Mapping erkannt werden,
- warum das vorhandene Cachelimit auf die Identity-Repräsentation wirkt,
- welche direkten Stream- und Legacy-Arbeiten bewusst für Phase 3 offenbleiben.

Der Abschlussbericht muss exakt mit folgendem Block enden:

```text
Changed files:
New files:
Acceptance criteria satisfied:
Tests / validation performed:
Open points / blockers:
```
