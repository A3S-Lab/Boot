import * as path from 'node:path';
import { defineConfig } from '@rspress/core';
import { versionAwareLinksPlugin } from './plugins/version-aware-links';
import { defaultVersion, versions } from './versions.mjs';

const base = process.env.DOCS_BASE ?? '/Boot/';
const siteOrigin = process.env.DOCS_ORIGIN ?? 'https://a3s-lab.github.io';

export default defineConfig({
  root: path.join(__dirname, '.generated-docs'),
  base,
  siteOrigin,
  title: 'A3S Boot',
  description:
    'An adapter-first modular Rust web framework with typed dependency injection, request pipelines, protocols, and technique modules.',
  lang: 'zh',
  icon: '/favicon.svg',
  logo: '/a3s-boot-mark.svg',
  logoText: 'A3S Boot',
  outDir: 'doc_build',
  llms: true,
  route: {
    localeRedirect: 'never',
  },
  multiVersion: {
    default: defaultVersion,
    versions,
  },
  plugins: [versionAwareLinksPlugin(__dirname)],
  locales: [
    {
      lang: 'zh',
      label: '简体中文',
      title: 'A3S Boot',
      description:
        '适配器优先的模块化 Rust Web 框架，提供类型化依赖注入、请求管线、多协议与技术模块。',
    },
    {
      lang: 'en',
      label: 'English',
      title: 'A3S Boot',
      description:
        'An adapter-first modular Rust web framework with typed dependency injection, request pipelines, protocols, and technique modules.',
    },
  ],
  head: [
    ['meta', { name: 'theme-color', content: '#f7f7f8' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'A3S Boot' }],
    [
      'meta',
      {
        property: 'og:image',
        content: `${siteOrigin}${base}social-card.svg`,
      },
    ],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    (route) => [
      'link',
      {
        rel: 'canonical',
        href: `${siteOrigin}${base.replace(/\/$/, '')}${route.routePath}`,
      },
    ],
  ],
  themeConfig: {
    search: true,
    enableContentAnimation: true,
    editLink: {
      docRepoBaseUrl:
        'https://github.com/A3S-Lab/Boot/tree/main/website/content',
    },
    lastUpdated: true,
    llmsUI: {
      placement: 'outline',
      viewOptions: ['markdownLink', 'chatgpt', 'claude'],
    },
    socialLinks: [
      {
        icon: 'github',
        mode: 'link',
        content: 'https://github.com/A3S-Lab/Boot',
      },
    ],
  },
});
