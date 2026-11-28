# EDM on a home server (Docker)

How to run EDM with no window on a NAS or a small computer with no screen, and give it
downloads from there.

1. Build the image from EDM's source folder:
   `docker build -f tooling/docker/Dockerfile -t EDM .`
   On a computer with little memory, add `--build-arg JOBS=2`.
2. Start it, keeping its list, settings and downloads in a volume:
   `docker run -d --name EDM --restart unless-stopped -v edm-data:/data EDM`
3. Give it a download: `docker exec EDM edm-gpui https://example.com/file.zip`
4. See what it holds: `docker exec EDM edm-gpui list`. Pause, resume or cancel one by the
   number `list` shows: `docker exec EDM edm-gpui pause 3`.
5. Finished files are in the volume under `EDM`, sorted by kind (`EDM/Documents`,
   `EDM/Videos`…). To have them in a folder of the server's own, start it with
   `-v /path/on/the/server:/data` instead of `edm-data:/data`.

An AI assistant can drive it too: `docker exec -i EDM edm-gpui mcp` speaks the Model Context
Protocol over its input and output.

EDM's control port stays inside the container: nothing is opened on the server's network, and
no page is served. Downloads keep going while EDM runs, and resume where they stopped after the
container restarts.
