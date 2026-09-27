# GRAŃ – atlas szlaków górskich 3D (self-hosting)
FROM node:22-alpine AS build
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
RUN npm run build && npm prune --omit=dev

FROM node:22-alpine
WORKDIR /app
ENV NODE_ENV=production PORT=5190 HOST=0.0.0.0 GRAN_DATA_DIR=/data GRAN_CACHE_DIR=/data/.cache
COPY --from=build /app/node_modules ./node_modules
COPY --from=build /app/dist ./dist
COPY package.json ./
COPY public ./public
COPY scripts ./scripts
COPY server ./server
VOLUME /data
EXPOSE 5190
CMD ["node", "server/index.mjs"]
