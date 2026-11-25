#!/usr/bin/env bash
# Qt Bridge requires Qt >= 6.10 with private headers available.
set -euo pipefail
sudo apt-get update
sudo apt-get install -y \
  build-essential \
  cmake \
  pkg-config \
  qt6-base-dev \
  qt6-declarative-dev \
  qt6-base-private-dev \
  qt6-declarative-private-dev \
  qt6-tools-dev
echo "qmake: $(qmake6 -query QT_VERSION 2>/dev/null || qmake -query QT_VERSION)"
