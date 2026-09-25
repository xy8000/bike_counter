# syntax=docker/dockerfile:1

# ---------- Build stage ----------
FROM node@sha256:ebfe2f90462722a7a4de65e91990e97fe0d401c70e0e762c5b53302f905ec1c1 AS builder
WORKDIR /build

# Install dependencies from the committed lockfile for reproducible builds.
COPY package.json package-lock.json ./
RUN npm ci --ignore-scripts

# Build the production bundle into /build/dist.
COPY . .
RUN npm run build

# ---------- Runtime stage ----------
FROM nginx@sha256:1ed1b0e1d7652937d6cbdaf4018c7b6fc009a7dd6c3047351e2eddda745de43f
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
