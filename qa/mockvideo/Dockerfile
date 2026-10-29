# Pages and media that yt-dlp's generic extractor reads, made once at build
# time with ffmpeg (no network when the suite runs).
FROM python:3.12-slim-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg && rm -rf /var/lib/apt/lists/*
COPY mockvideo/make.sh /make.sh
RUN sh /make.sh /site
COPY mockvideo/serve.py /serve.py
EXPOSE 8090
CMD ["python3", "-u", "/serve.py"]
