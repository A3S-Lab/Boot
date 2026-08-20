export const defaultVersion = 'v0.2.0';

export const versionDefinitions = [
  {
    id: 'v0.2.0',
    release: '0.2.0',
    capabilities: ['orm-0-3'],
  },
  {
    id: 'v0.1.4',
    release: '0.1.4',
    capabilities: [],
  },
];

export const versions = versionDefinitions.map(({ id }) => id);
