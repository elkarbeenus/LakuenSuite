# LakuenSuite

Alle Apps aus den GitHub-Repos von **elkarbeenus** an einem Ort: anzeigen, installieren, aktualisieren und starten. Als AppImage für SteamOS (Desktop-Modus) und andere Linux-Systeme.

![Logo](src/logo.png)

## So funktioniert's

- Beim Öffnen liest LakuenSuite alle Repos von `github.com/elkarbeenus`.
- **Repo mit Release:** farbige Kachel. Klick öffnet ein Fenster mit kurzer Beschreibung, Versionsinfo und den Knöpfen **Installieren**, **Update** oder **Starten**.
- **Repo ohne Release:** ausgegraute Kachel. Sie wird aktiv, sobald ein Release mit AppImage veröffentlicht ist.
- Release ohne Linux-AppImage: Kachel aktiv, aber mit Hinweis „kein Linux-AppImage“.

## Funktionen

- Installieren, Aktualisieren, Starten, Deinstallieren mit einem Klick
- **Automatische Updates** für installierte Apps beim Start und im Hintergrund (abschaltbar)
- **Selbst-Update:** LakuenSuite prüft sein eigenes neuestes Release und ersetzt sich auf Knopfdruck (oder automatisch), startet neu, Einstellungen bleiben
- Jeder Download wird per **SHA-256** geprüft (GitHub-Digest oder `.sha256`/`SHA256SUMS` im Release)
- Vorherige Version wird aufgehoben → **Zurücksetzen** möglich; danach pausieren Auto-Updates bis zur nächsten neuen Version
- Fortschritt mit Geschwindigkeit und Restzeit, Abbrechen jederzeit, Downloads in Warteschlange
- Echte App-Icons (aus dem AppImage gelesen) und **Startmenü-Einträge** (abschaltbar)
- Suche, Filter (Installiert / Updates / Verfügbar / Favoriten / Ohne Release), Sortierung, Raster- oder Listenansicht
- Favoriten ★, Repos ausblenden, Forks/Archivierte ein- oder ausblenden
- Release-Notizen („Was ist neu“) im App-Fenster
- Anzeige, welche App gerade läuft; Startzähler und „zuletzt gestartet“
- Offline-Modus: letzter Stand bleibt sichtbar, installierte Apps starten trotzdem
- Schonender Umgang mit dem GitHub-Limit (ETag-Cache); optional eigener Token (5000 Abfragen/h, private Repos)
- 7 Themes (6 dunkle, 1 helles), Tastatur- und Controller-Bedienung (Pfeiltasten, Enter, Esc, Strg+F, F5)
- Einführung beim ersten Start; keine Anmeldung, keine Datensammlung

## Optionale Kurzbeschreibung pro Repo

Lege in einem App-Repo eine Datei `.lakuensuite.json` an, um Namen und Kurztext im Popup festzulegen:

```json
{
  "name": "LakuenCast",
  "short": "Überträgt Bild und Ton auf deinen Chromecast – mit eigener Oberfläche.",
  "category": "Medien"
}
```

Ohne die Datei nimmt LakuenSuite die Repo-Beschreibung und den ersten Absatz der README (gekürzt).

## Speicherorte

| Was | Wo |
|---|---|
| Apps | `~/Applications/LakuenSuite/<Repo>/<Repo>.AppImage` (änderbar) |
| Einstellungen | `~/.config/lakuensuite/settings.json` |
| Installationsstand, Icons | `~/.local/share/lakuensuite/` |
| GitHub-Cache | `~/.cache/lakuensuite/` |

`LAKUENSUITE_NO_UPDATE=1` schaltet die Selbst-Update-Prüfung ab.

## Bauen

GitHub Actions baut bei jedem Push ein AppImage (Artefakt). Ein Tag `v*` veröffentlicht ein Release mit `LakuenSuite-x86_64.AppImage` und `.sha256`; Notizen kommen aus `release-notes/<tag>.md`.

Lokal: `npm install && npx tauri build --bundles appimage` (Rust, Node und die Tauri-Linux-Abhängigkeiten vorausgesetzt).

---

Erstellt von Dominic Lesina x Claude
