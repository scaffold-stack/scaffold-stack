import type { NextConfig } from 'next';

const nextConfig: NextConfig = {
  reactStrictMode: true,
  async rewrites() {
    return [
      {
        // Browser calls stay same-origin. Stacks core on :20443 often has no CORS.
        source: '/devnet-node/:path*',
        destination: 'http://127.0.0.1:20443/:path*',
      },
    ];
  },
};

export default nextConfig;

