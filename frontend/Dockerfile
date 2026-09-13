# syntax=docker/dockerfile:1

# ---------- Build stage ----------
FROM node:24-alpine@sha256:333f6b3eca25980d5682c26207665b93c9417786b21760b2764d5821d9704c8a AS builder
WORKDIR /build

# Install dependencies from the committed lockfile for reproducible builds.
COPY package.json package-lock.json ./
RUN npm ci --ignore-scripts

# Build the production bundle into /build/dist.
COPY . .
RUN npm run build

# ---------- Runtime stage ----------
FROM nginx:1.31-alpine@sha256:bd43d3d8fdbbe597c36fea489886796466a9ea5b662e8c50897d53e190c88a3d
# Serve the built SPA and reverse-proxy /api to the backend service. nginx.conf
# is a template: the official nginx entrypoint substitutes `${...}` env vars
# from /etc/nginx/templates into conf.d.
COPY nginx.conf.template /etc/nginx/templates/default.conf.template
COPY --from=builder /build/dist /usr/share/nginx/html

EXPOSE 80
