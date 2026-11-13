FROM nginxinc/nginx-unprivileged:stable-alpine

COPY index.html styles.css app.js repository.js theme.js /usr/share/nginx/html/
COPY assets/ /usr/share/nginx/html/assets/

EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s \
  CMD wget -q -T 3 -O /dev/null http://127.0.0.1:8080/ || exit 1
