# syntax=docker/dockerfile:1

# ---------- Build stage ----------
FROM node:24-alpine AS builder
WORKDIR /build

# Install dependencies from the committed lockfile for reproducible builds.
COPY package.json package-lock.json ./
RUN npm ci

# Build the production bundle into /build/dist.
COPY . .
RUN npm run build

# ---------- Runtime stage ----------
FROM nginx:1.31-alpine
# Serve the built SPA and reverse-proxy /api to the backend service.
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /build/dist /usr/share/nginx/html

EXPOSE 80
