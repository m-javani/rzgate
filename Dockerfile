# // SPDX-License-Identifier: BUSL-1.1
# // Copyright (c) 2026 M. Javani
# //
# // This file is part of rzgate.
# //
# // Use of this software is governed by the Business Source License 1.1
# // included in the LICENSE file in the root of this repository.

FROM ubuntu:24.04

RUN apt-get update && apt-get install -y ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

RUN mkdir -p /opt/rzgate/certs /opt/rzgate/configs

# Binary is copied to root by CI
COPY rzgate /opt/rzgate/rzgate

RUN chmod +x /opt/rzgate/rzgate

EXPOSE 8777 3443

CMD ["/opt/rzgate/rzgate"]