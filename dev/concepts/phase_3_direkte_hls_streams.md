# Coding-Agent-Auftrag: Phase 3 – Direkte HLS-Streams

Implementiere ausschließlich **Phase 3** der Tuliprox-Anpassung für HTTP-`Content-Encoding` bei HLS.

Die bindende technische Spezifikation ist:

```text
dev/concepts/hls-content-encoding/hls_content_encoding_policy_and_processing_concept.md
```

Maßgeblich sind insbesondere die Regeln für Shared-Key-/Transient-Passthrough, den Legacy-Provider-Repräsentationsmodus,
sichere Response-Header, `PreserveOrigin`, `mark_response_as_uncompressed`, pfadspezifische Retry-Grenzen,
Modus-Propagation sowie **Phase 3** im Abschnitt „Implementierungsphasen gemäß `AGENTS.md`“.

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

   Ein in der aktuellen `AGENTS.md` genannter anderer Pfad hat Vorrang.
5. Prüfe, ob die Ergebnisse aus Phase 1 und Phase 2 vollständig vorhanden sind. Verifiziere insbesondere:
   - den zentralen Content-Coding-Layer,
   - finales `Accept-Encoding: identity` für HLS-Requests,
   - die dekodierte Shared-Resource-Grenze,
   - typisierte Decoderfehler,
   - die unveränderte bestehende Range-Policy.

Fehlende Voraussetzungen aus Phase 1 oder 2 dürfen nicht stillschweigend in diesen Auftrag aufgenommen werden.
Dokumentiere sie als Blocker oder beschränke dich auf unabhängige Arbeiten. Fehlt eine bindende Spezifikation, darf ihr
Inhalt nicht geraten werden.

## Arbeitsmodus

Arbeite zuerst im Plan-Modus. Der Plan muss enthalten:

1. zu ändernde und neu anzulegende Dateien,
2. überprüfte Voraussetzungen aus Phase 1 und 2,
3. exakten Scope dieser Phase,
4. Tests und lokale Testserver-Fixtures,
5. Risiken und Annahmen,
6. bewusst ausgeschlossene Cleanup- und Observability-Arbeiten aus Phase 4.

Beginne erst danach mit der Implementierung. Halte Änderungen klein, reviewbar und kompilierbar.

## Scope dieser Phase

Setze ausschließlich die folgenden Arbeitspakete um:

1. Stelle Shared-Key- und nicht cachefähige Transient-Passthrough-Pfade auf die zentrale dekodierte Shared-Response um.
2. Sorge dafür, dass Key-, Map-, Part- und sonstige direkte Shared-HLS-Ressourcen ausschließlich als
   Identity-Repräsentation an den Client gestreamt werden.
3. Verwende bei Binärressourcen ausschließlich deklarierte `Content-Encoding`-Informationen. Führe kein Magic-Sniffing
   für Keys, Segmente, Maps, Parts oder sonstige Binärdaten ein.
4. Entferne nach tatsächlicher Dekompression veraltete repräsentationsabhängige Header gemäß Konzept. Verwende weiterhin
   nur eine enge, sichere Client-Response-Header-Auswahl.
5. Markiere alle direkten binären Shared-HLS-Responses mit dem bestehenden `mark_response_as_uncompressed`, damit Tower
   keine zweite Kompression anwendet.
6. Dokumentiere und implementiere die korrekte Streaming-Fehlergrenze: Ein Decoderfehler nach Beginn der Client-Response
   beendet den Stream. Simuliere keinen transparenten Retry nach gesendeten Response-Headern.
7. Führe für Legacy-Provider-Streams den im Konzept beschriebenen schmalen fachlichen Modus ein, beispielsweise:

   ```rust
   ProviderContentRepresentationMode::PreserveOrigin
   ProviderContentRepresentationMode::HlsIdentity
   ```

   Verwende die zum aktuellen Branch passende Benennung, ohne eine generische Proxy-Policy-Plattform aufzubauen.
8. Lass der Legacy-Modus zusammenhängend steuern:
   - finales `Accept-Encoding: identity` beim HLS-Origin-Request,
   - Dekompression der Provider-Response nach Identity,
   - sichere Auswahl repräsentationskonsistenter Client-Response-Header.
9. Propagiere den Modus explizit durch alle relevanten Legacy-Pfade: initialer Provider-Open, deferred Provider-Open,
   Reconnect, Provider-Wechsel sowie HLS-Catchup.
10. Setze `HlsIdentity` für alle Legacy-HLS-Ressourcen, einschließlich Segment, Map, Key, Part und sonstigen
    umgeschriebenen URI-Ressourcen. Leite dies nicht ausschließlich aus `PlaylistItemType::LiveHls` ab, da HLS-Catchup
    anders klassifiziert sein kann.
11. Erhalte für Nicht-HLS-Provider-Streams den `PreserveOrigin`-Modus:
    - Body bleibt bytegenau unverändert,
    - `Content-Encoding` bleibt zusammen mit dem unveränderten Body erhalten,
    - `Content-Length` und `Content-Range` bleiben konsistent,
    - `Transfer-Encoding` wird nicht in eine neu erzeugte Axum-Response kopiert.
12. Verwende keine pauschale End-to-End-Headerkopie und gib keine internen Provider-Cookies, Authentifizierungs- oder
    proprietären Header global frei.
13. Markiere Legacy-HLS-Binärresponses gegen Tower-Doppelkompression.
14. Erhalte bestehende Backpressure-, Client-Send-, Body- und Idle-Deadlines. Puffere keine beliebig großen Segmente
    vollständig.
15. Kleine Key-Bodies dürfen nur dann bewusst begrenzt gepuffert werden, wenn dies im aktuellen Codepfad eine klare,
    getestete Retry- oder Header-Korrektheitsverbesserung bietet und das Konzept sowie `AGENTS.md` eingehalten werden.
    Führe keine allgemeine Vollpufferung ein.

## Explizit nicht enthalten

Implementiere in dieser Phase nicht:

- eine neue gelabelte Metrikarchitektur,
- allgemeine Proxy-Header- oder Response-Frameworks,
- pauschales Kopieren aller End-to-End-Header,
- reqwest-Autodekompression,
- eine zweite Range-Policy,
- Vollpufferung großer Medienobjekte,
- Cleanup sämtlicher alter Decoderduplikate, sofern es nicht zwingend für diese Phase erforderlich ist,
- allgemeine Umbenennungs- oder Modulverschiebungsrefactorings,
- Änderungen an HLS-Inhaltsverschlüsselung oder Key-Material,
- Änderungen an nicht betroffenen Nicht-HLS-Streamsemantiken.

Phase-4-Arbeiten sind unter „Open points / blockers“ zu dokumentieren und nicht vorzuziehen.

## Akzeptanzkriterien

Die Phase gilt nur dann als abgeschlossen, wenn mindestens Folgendes nachweisbar ist:

- Komprimierte Shared-Keys werden als exakte dekodierte Key-Bytes ausgeliefert.
- Direkte Shared-Map-, Part- und Other-Ressourcen werden nach Identity dekodiert.
- Ein binärer Body ohne `Content-Encoding`, der zufällig mit einer Kompressionssignatur beginnt, bleibt unverändert.
- Transformierte direkte Responses enthalten kein veraltetes `Content-Encoding` und keine falsche `Content-Length`.
- Direkte binäre Shared-HLS-Responses sind mit `mark_response_as_uncompressed` markiert.
- Streamzeitige Decoderfehler führen zu einem sauberen Streamabbruch und nicht zu einem fingierten Retry.
- Legacy-HLS-Segment, Map, Key, Part und Other werden als Identity ausgeliefert.
- HLS-Catchup setzt explizit den HLS-Identity-Modus.
- Der Modus bleibt bei initialem Open, deferred Open, Reconnect und Provider-Wechsel erhalten.
- Ein codiertes `206 Partial Content` wird gemäß zentraler Decoderregel abgelehnt.
- Nicht-HLS-`PreserveOrigin` erhält Body und `Content-Encoding` konsistent.
- `Transfer-Encoding` wird nicht in neu erzeugte Client-Responses kopiert.
- Keine internen oder sensiblen Provider-Header werden durch eine zu breite Header-Policy freigegeben.
- Legacy- und Shared-HLS-Binärresponses werden nicht durch Tower doppelt komprimiert.

## Tests

Implementiere die Phase-3-relevanten Tests aus dem Konzept. Dazu gehören mindestens:

### Shared direkt

- gzip-/Brotli-/Zstandard-Key wird als exakte dekodierte Bytefolge ausgeliefert,
- komprimierte Map-, Part- und Other-Passthrough-Response,
- keine Magic-Erkennung bei Binärressourcen,
- zufällige gzip-Magic in einem unkomprimierten Key bleibt unverändert,
- Headernormalisierung nach Dekompression,
- `mark_response_as_uncompressed`,
- streamzeitiger Decoderfehler beendet den Stream ohne transparenten Retry.

### Legacy HLS

- komprimiertes Segment wird dekodiert,
- komprimierter Key wird dekodiert,
- komprimierte Map wird dekodiert,
- Part/Other folgen derselben Identity-Policy,
- codiertes `206` wird abgelehnt,
- HLS-Catchup setzt den Modus,
- initialer und deferred Provider-Open behalten den Modus,
- Reconnect und Provider-Wechsel behalten den Modus,
- Streamdecoderfehler führt zu Streamabbruch,
- Binärresponse ist gegen Tower-Doppelkompression markiert.

### Nicht-HLS Preserve

- Body bleibt bytegenau erhalten,
- `Content-Encoding` wird zusammen mit dem Body weitergegeben,
- `Content-Length` und `Content-Range` bleiben konsistent,
- `Transfer-Encoding` wird nicht übernommen,
- keine unbeabsichtigte Dekompression oder Magic-Erkennung.

Nutze ausschließlich lokale Testserver und bestehende Repository-Testpatterns.

## Codequalität und DRY

- Verwende den Decoder und die dekodierte Shared-Response aus den vorigen Phasen.
- Führe nur einen schmalen Legacy-Repräsentationsmodus ein.
- Vermeide generische Framework-Abstraktionen und pauschale Headerkopien.
- Halte sichere Client-Response-Header explizit und kontextabhängig.
- Propagiere den Legacy-Modus vollständig, aber ohne duplizierte Fallunterscheidungen an jedem Callsite. Nutze
  bestehende Factory- und Options-Strukturen sinnvoll.
- Vermeide positionalen `bool`-Parameter.
- Erhalte streamende Backpressure; puffere große Bodies nicht vollständig.
- Verwende `mark_response_as_uncompressed` ausschließlich für binäre beziehungsweise unverändert zu streamende
  HLS-Responses, nicht für neu erzeugte Manifeste.

## Validierung

Führe zunächst fokussierte Tests und anschließend die Gesamtprüfungen aus. Erwartet werden insbesondere:

```text
cargo +stable test -p tuliprox hls
cargo +stable test -p tuliprox provider_stream
cargo +stable clippy -p tuliprox -- -D warnings
make fmt-check
make lint
make test
```

Falls Markdown-Dokumente geändert werden:

```text
make markdown-lint
```

Dokumentiere nicht ausführbare oder fehlgeschlagene Befehle exakt.

## Abschlussprüfung und Bericht

Validiere die fertige Phase erneut gegen:

1. alle anwendbaren `AGENTS.md`,
2. Phase 3 des Content-Encoding-Konzepts,
3. die sichere Header- und Preserve-Origin-Semantik,
4. alle Legacy-Open-/Reconnect-Pfade,
5. die laut Root-`AGENTS.md` bindende allgemeine HLS-Zielspezifikation.

Berichte zusätzlich kurz:

- wie der Legacy-HLS-Modus durch die Factory-/Reconnect-Kette propagiert wird,
- welche Header im Preserve- und HLS-Identity-Modus erhalten beziehungsweise entfernt werden,
- wo direkte Streaming-Decoderfehler sichtbar werden und warum kein transparenter Retry möglich ist,
- welche Cleanup- und Observability-Arbeiten bewusst für Phase 4 verbleiben.

Der Abschlussbericht muss exakt mit folgendem Block enden:

```text
Changed files:
New files:
Acceptance criteria satisfied:
Tests / validation performed:
Open points / blockers:
```
