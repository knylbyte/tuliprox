ARG ALPINE_VERSION=3.22.1

FROM alpine:${ALPINE_VERSION}

RUN apk add --no-cache ffmpeg
WORKDIR /src
COPY resources ./resources

# Combine ffmpeg commands into a single layer to reduce image size
RUN ffmpeg -loop 1 -i ./resources/channel_unavailable.jpg -t 10 -r 1 -an \
    -vf "scale=1920:1080" \
    -c:v libx264 -preset veryfast -crf 23 -pix_fmt yuv420p \
    ./resources/channel_unavailable.ts && \
  ffmpeg -loop 1 -i ./resources/user_connections_exhausted.jpg -t 10 -r 1 -an \
    -vf "scale=1920:1080" \
    -c:v libx264 -preset veryfast -crf 23 -pix_fmt yuv420p \
    ./resources/user_connections_exhausted.ts && \
  ffmpeg -loop 1 -i ./resources/provider_connections_exhausted.jpg -t 10 -r 1 -an \
    -vf "scale=1920:1080" \
    -c:v libx264 -preset veryfast -crf 23 -pix_fmt yuv420p \
    ./resources/provider_connections_exhausted.ts && \
   ffmpeg -loop 1 -i ./resources/user_account_expired.jpg -t 10 -r 1 -an \
     -vf "scale=1920:1080" \
     -c:v libx264 -preset veryfast -crf 23 -pix_fmt yuv420p \
     ./resources/user_account_expired.ts