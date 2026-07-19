import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  output: 'export',
  basePath: '/app',
  assetPrefix: '/app/',
  images: {
    unoptimized: true,
  },
}

export default nextConfig
