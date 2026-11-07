# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

FROM python:3.12-slim-bookworm
RUN pip install --no-cache-dir cryptography==43.0.3
COPY lib /qa/lib
COPY origin/origin.py /origin.py
EXPOSE 8080 8443 8444 8445 8446
CMD ["python3", "-u", "/origin.py"]
