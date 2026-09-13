# syntax=docker/dockerfile:1

# ---------- Build stage ----------
FROM node@sha256:333f6b3eca25980d5682c26207665b93c9417786b21760b2764d5821d9704c8a AS builder
WORKDIR /build

# Install dependencies from the committed lockfile for reproducible builds.
COPY package.json package-lock.json ./
RUN npm ci --ignore-scripts

# Build the production bundle into /build/dist.
COPY . .
RUN npm run build

# ---------- Runtime stage ----------
FROM nginx@sha256:bd43d3d8fdbbe597c36fea489886796466a9ea5b662e8c50897d53e190c88a3d
# Run as the image's unprivileged `nginx` user. The official entrypoint renders
# the template into /etc/nginx/conf.d and the worker writes its temp files to
# /var/cache/nginx, so both must be owned by `nginx`. The worker binds the
# unprivileged port 8080 (see nginx.conf.template).
RUN chown -R nginx:nginx /etc/nginx/conf.d /var/cache/nginx \
    && touch /run/nginx.pid \
    && chown nginx:nginx /run/nginx.pid
# Serve the built SPA and reverse-proxy /api to the backend service. nginx.conf
# is a template: the official nginx entrypoint substitutes `${...}` env vars
# from /etc/nginx/templates into conf.d.
COPY nginx.conf.template /etc/nginx/templates/default.conf.template
COPY --from=builder /build/dist /usr/share/nginx/html

USER nginx
EXPOSE 8080
