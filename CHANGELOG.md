# Changelog

## [1.1.0](https://github.com/mulhamna/trello-commands/compare/v1.0.1...v1.1.0) (2026-10-16)


### Features

* **cli:** add per-profile board access scopes ([2eecc15](https://github.com/mulhamna/trello-commands/commit/2eecc15c091fbbaf398b17f307a3a74f4985d0ec))
* **cli:** add per-profile board access scopes ([0c1c708](https://github.com/mulhamna/trello-commands/commit/0c1c70864a83122b293c8b44244d82e1b8c8e667))


### Bug Fixes

* **cli:** avoid logging board identifiers ([493fa69](https://github.com/mulhamna/trello-commands/commit/493fa69c2d45de61e625725fc8cd90e5a4c2e381))
* **core:** avoid unused Windows test binding ([3b99a1a](https://github.com/mulhamna/trello-commands/commit/3b99a1a79cd985c8b9b35306bb68a969ef06000f))

## [1.0.1](https://github.com/mulhamna/trello-commands/compare/v1.0.0...v1.0.1) (2026-10-15)


### Miscellaneous Chores

* release 1.0.1 ([b683a94](https://github.com/mulhamna/trello-commands/commit/b683a947cde4873664cae72f6c694c04e4c89e37))

## 1.0.0 (2026-10-15)


### Features

* add trelloc CLI and MCP server v1 ([df21922](https://github.com/mulhamna/trello-commands/commit/df21922096c0045c6ec7ccf6454a778611f10be4))
* **cli:** add auth, hierarchy, config and api commands ([27583b6](https://github.com/mulhamna/trello-commands/commit/27583b6c6049e79ce80e59df8ea2a9212b3822ef))
* **cli:** add bulk, batch, board/list management and reports ([0318aef](https://github.com/mulhamna/trello-commands/commit/0318aeffef8dcfbe2febbf397e1f73b555ac6a33))
* **cli:** add card commands ([656c8cb](https://github.com/mulhamna/trello-commands/commit/656c8cb907d234f6d489898edf317f9d4210b6af))
* **cli:** add comment, checklist, label, field and attachment commands ([2604c22](https://github.com/mulhamna/trello-commands/commit/2604c22a9683fdfc2eeadcff0cf44bee6ba44c07))
* **cli:** add mcp install and doctor via kurir ([c315e60](https://github.com/mulhamna/trello-commands/commit/c315e6069acc8fbaf2e361d9aaf74ae5f8fa3a0e))
* **core:** add board and list writes and a bounded bulk runner ([97b6d95](https://github.com/mulhamna/trello-commands/commit/97b6d955add6d496c95ceb673aebf254f6ce12c8))
* **core:** add bulk, batch, board/list management and reports ([dd556fc](https://github.com/mulhamna/trello-commands/commit/dd556fcc20dd2ccbf5f8b3ac98c60ce2c1c626ce))
* **core:** add card and collaboration endpoints ([ba4710f](https://github.com/mulhamna/trello-commands/commit/ba4710fe3f28b054c5a28931cfc5603107204a6b))
* **core:** add card and collaboration operations ([ba5d3a3](https://github.com/mulhamna/trello-commands/commit/ba5d3a3ad5072b887fd24240da4a6c44777e26c8))
* **core:** add card refs, card models and filter planner ([e7c5784](https://github.com/mulhamna/trello-commands/commit/e7c5784a60d10ad804b62d72a8f6851e62e69991))
* **core:** add HTTP client with OAuth header, batch and raw calls ([c1e43ac](https://github.com/mulhamna/trello-commands/commit/c1e43ac8f719d7a935f233b9663161584ad42fc4))
* **core:** add profile config, error type and date parsing ([d1deb99](https://github.com/mulhamna/trello-commands/commit/d1deb99a82e5cc8d1a3ed86222d822c60a83fe7e))
* **core:** add shared hierarchy operations ([df20b5c](https://github.com/mulhamna/trello-commands/commit/df20b5c34591b457ddcf2fe195a2b1bc54c9a4c2))
* **core:** pace requests with rolling-window rate limiter ([8f17df4](https://github.com/mulhamna/trello-commands/commit/8f17df433b667aaf72738f8cff179ab044473385))
* **core:** resolve board, list, label, member and field names ([195407a](https://github.com/mulhamna/trello-commands/commit/195407a8dfb6691311024e6ff37086f2d9d6b5c6))
* **mcp:** add auth, hierarchy and raw request tools ([ed0edde](https://github.com/mulhamna/trello-commands/commit/ed0eddef1fedf20c63af37c59151c757b2fd15d3))
* **mcp:** add bulk, batch, copy, board/list and report tools ([03e003a](https://github.com/mulhamna/trello-commands/commit/03e003a0a1e5d1091034aac62fbe7ef4342312e5))
* **mcp:** add card tools ([0de1873](https://github.com/mulhamna/trello-commands/commit/0de187305fc2da68b4a553e11e5df678aad9d72a))
* **mcp:** add comment, checklist, label, field and attachment tools ([862cb60](https://github.com/mulhamna/trello-commands/commit/862cb60a2b1d484848bd3a035a932a4eb4e28abf))


### Code Refactoring

* **core:** rename card_numbers to short_ids ([dd6d00e](https://github.com/mulhamna/trello-commands/commit/dd6d00e20a86e9a2e0a9ac66b12ec5011f31d538))
