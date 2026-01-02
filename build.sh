#!/usr/bin/env bash
# // SPDX-License-Identifier: BUSL-1.1
# // Copyright (c) 2026 M. Javani
# //
# // This file is part of rzgate.
# //
# // Use of this software is governed by the Business Source License 1.1
# // included in the LICENSE file in the root of this repository.

set -euo pipefail

# cargo clean

cargo build  --release

strip --strip-all target/release/rzgate

upx --best --lzma target/release/rzgate

ls -lh target/release/rzgate

cp target/release/rzgate .
