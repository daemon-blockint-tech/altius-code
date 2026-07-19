import type { Metadata } from 'next'
import './globals.css'

export const metadata: Metadata = {
  title: 'Altius Fleet',
  description: 'Multi-agent SVM development fleet',
  manifest: '/manifest.webmanifest',
}

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="anonymous" />
        <link
          href="https://fonts.googleapis.com/css2?family=Lora:ital,wght@0,400;0,500;0,600;1,400&family=Poppins:wght@600;700&display=swap"
          rel="stylesheet"
        />
        <meta name="theme-color" content="#141413" />
      </head>
      <body className="antialiased">{children}</body>
    </html>
  )
}
