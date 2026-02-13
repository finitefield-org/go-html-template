# go-html-template 互換 TODO（html/ 実装比較）

## 優先度: 高
- [x] `ParseFS` / `Template::parse_fs` 相当の `FS` 抽象（`fs.FS`）対応を追加する
  - Go 側: `ParseFS(fs fs.FS, patterns ...string)` / `Template.ParseFS(...)`
- [x] `AddParseTree` 相当を追加する
  - Go 側の `AddParseTree(name string, tree *parse.Tree)` が未実装
- [x] 名前空間共有付きの `New(name string)` メソッド相当を追加する
  - Rust 側は `Template::new` は存在するが、既存テンプレート集合に所属する `New` API がない
- [x] `Clone` API を Go 的に再実装する
  - `clone_template` は存在するが、Go の `Clone` と同等の名前空間・`Tree` 結合・実行後制約と完全一致していない
- [x] `Templates()` / `DefinedTemplates()` の API 互換を追加する
  - 現状は `templates()` と `defined_templates` 系だが、公開シグネチャと戻り値仕様が異なる
- [x] `Template::nameSpace` 相当の共有状態や `Tree` 参照を含む型整合を整理する（必要なら `Tree` フィールド公開/参照 API を追加）

## エラー/診断
- [x] `TemplateError` の体系を `ErrorCode` 化し、Go 的なカテゴリ（`ErrBadHTML`, `ErrBranchEnd`, `ErrAmbigContext`, `ErrOutputContext` など）を持つ
- [x] 可能なら `line/名前/原因` などの診断情報を保持し、`Error()` の文言をパース可能な範囲で互換化する

## コンテキスト解析・エスケープ
- [x] Go の `transition`/`context` 方式に近い完全な状態遷移へ拡張する
  - JS 正規表現・テンプレートリテラル・コメント状態、CSS URL・CSS 文字列/コメント状態など
  - 本タスク: `ScriptRegexp` / `ScriptTemplate` / JS/CSSコメント状態の状態遷移反映を完了
- [x] `js`/`css`/`html` 文脈エスケープの不足分を追加する
  - `ScriptTemplate` の `` ` `` / `${` エスケープ追加
  - `ScriptLineComment` / `ScriptBlockComment` / `StyleLineComment` / `StyleBlockComment` を `seed` 及び `placeholder` に対応
- [x] `attrTypeMap` 相当を導入し、属性種別を厳密化する
  - 現状は簡易判定のため、Go の `attrType`/`attrTypeMap` 由来の境界条件を取り込めていない
- [x] `srcset` エスケープを `srcsetFilterAndEscaper` と同等実装にする
  - エントリ分割・メタデータ許容の判定・不正値の `#ZgotmplZ` 代替
- [x] 属性名位置での `htmlNameFilter` 的挙動を追加する
  - 属性名インジェクション耐性の観点で必要
  - 動的属性名を `alnum + attr_type::plain` 条件でフィルタ、空文字/不正値は `#ZgotmplZ` に置換
- [x] `url` 扱いと安全URL判定を Go 互換へ再確認する
  - 特に URI スキーム許可/拒否の境界、正規化挙動の差分
- [x] `js`/`css`/`html` 用エスケープ処理を Go 側の既知のルール（文字列境界、`script` タグ特殊ケース等）へ寄せる
  - テスト追加: ScriptTemplate/ScriptRegexp/JS/CSSコメント状態

## 実行時データ・評価
- [x] `lookup_identifier` / `lookup_path` の評価モデルを text/template 的に再設計する
  - 現在は主に JSON 値前提で、Go 版の詳細なリフレクションベース解決と異なる
- [x] メソッド解決とフィールド解決の優先順・未定義時挙動を Go 準拠で再点検する
- [x] 再帰実行・深さ制御など（`text/template` 由来）を確認し、既知の実行限界テストを追加する

## テスト整備
- [x] Go 側 `html/template` の重要テスト群を移植して回帰を防ぐ
  - 移植済み: `template_test.go` の `TestStringsInScriptsWithJsonContentTypeAreCorrectlyEscaped` 相当
