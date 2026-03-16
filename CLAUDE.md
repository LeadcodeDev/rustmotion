# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.
See .claude/skills/ for detailed instructions on generating rustmotion scenarios.

## Règle obligatoire

Tout JSON de scénario généré doit être validé avec `rustmotion validate` avant d'être présenté à l'utilisateur.

## Encodage

- ffmpeg est auto-détecté et utilisé par défaut (10-bit H.264, meilleure qualité sur les gradients sombres)
- Sans ffmpeg, le fallback openh264 intégré encode en 8-bit
- Pour les vidéos avec des gradients sombres, recommander `--codec prores` pour une qualité maximale
