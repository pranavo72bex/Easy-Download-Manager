FROM python:3.12-slim-bookworm
RUN pip install --no-cache-dir cryptography==43.0.3
COPY lib /qa/lib
COPY origin/origin.py /origin.py
EXPOSE 8080 8443 8444 8445 8446
CMD ["python3", "-u", "/origin.py"]
