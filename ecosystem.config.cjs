module.exports = {
  apps: [
    {
      name: 'magpie',
      script: 'src/main.ts',
      interpreter: 'bun',

      // Working directory (adjust if deploying elsewhere)
      cwd: './',

      // Restart policy
      autorestart: true,
      watch: false,          // Use false in production; bun --watch is for dev
      max_restarts: 10,
      restart_delay: 5000,   // 5 seconds between restarts
      min_uptime: '10s',     // Must stay up at least 10s to count as "started"

      // Environment
      env: {
        NODE_ENV: 'production',
      },

      // Logging
      out_file: './logs/magpie-out.log',
      error_file: './logs/magpie-error.log',
      log_date_format: 'YYYY-MM-DD HH:mm:ss Z',
      merge_logs: true,
    },
  ],
};
