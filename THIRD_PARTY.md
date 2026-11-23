# Third-party notices

## Dism++ language files and rule data (Chuyu-Team/Dism-Multi-language)
Source: <https://github.com/Chuyu-Team/Dism-Multi-language>
Used as: translation memory for `i18n/` (ADR-0005) and seed data for rule packs converted from `Data.xml` (TD-0010).
Only data files are used. No Dism++ program code is used (clean-room rule, Vision §6).

```
MIT License

Copyright (c) 2016 Chuyu-Team

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## AccessKit's Windows adapter (AccessKit/accesskit, `adapters/windows` 0.35.2)
Source: <https://github.com/AccessKit/accesskit> at commit `f3026222559c7f5f46d8f25cb7404de68eb149f0`, the crates.io release `accesskit_windows` 0.35.2.
Used as: source code, vendored in `vendor/accesskit_windows/` with DISM Pro's changes. `vendor/accesskit_windows/PATCHES.md` lists them, and the code marks them `dpro:` (TD-0009 A2.5).
Licence: MIT OR Apache-2.0, at your option. Copyright the AccessKit authors. The texts are `vendor/accesskit_windows/LICENSE-MIT` and `LICENSE-APACHE`. `src/node.rs` is also derived from Chromium's accessibility code (copyright the Chromium authors, BSD-style), under `vendor/accesskit_windows/LICENSE.chromium`. DISM Pro's changes to these files are under the same terms.

Rust crate dependencies are listed in `THIRD_PARTY-crates.md`, generated at release by `xtask licenses` (cargo-about, in Docker).
