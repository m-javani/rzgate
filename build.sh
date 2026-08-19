#!/usr/bin/env bash
# // SPDX-License-Identifier: BUSL-1.1
# // Copyright (c) 2026 M. Javani
# //
# // This file is part of rzproxy.
# //
# // Use of this software is governed by the Business Source License 1.1
# // included in the LICENSE file in the root of this repository.

set -euo pipefail

# cargo clean

cargo build  --release

strip --strip-all target/release/rzproxy

upx --best --lzma target/release/rzproxy

ls -lh target/release/rzproxy

cp target/release/rzproxy .
