import { S3Client, PutObjectCommand, GetObjectCommand } from '@aws-sdk/client-s3';
import { getSignedUrl } from '@aws-sdk/s3-request-presigner';
import { Settings } from '../settings';
import { readdirSync, statSync, existsSync } from 'fs';
import { join, relative, basename } from 'path';
import { getCachedPresignedUrl, cachePresignedUrl } from './db';

let s3ClientInstance: S3Client | null = null;

export function getS3Client(settings: Settings): S3Client {
  if (s3ClientInstance) return s3ClientInstance;

  const s3Config = settings.s3;
  if (!s3Config.enabled || !s3Config.access_key || !s3Config.secret_key || !s3Config.bucket) {
    throw new Error('S3 is not fully configured or enabled.');
  }

  const clientConfig: any = {
    credentials: {
      accessKeyId: s3Config.access_key,
      secretAccessKey: s3Config.secret_key,
    },
    region: s3Config.region || 'us-east-1',
  };

  if (s3Config.endpoint) {
    clientConfig.endpoint = s3Config.endpoint;
    clientConfig.forcePathStyle = true; // Required for MinIO, rustfs, and custom providers
  }

  s3ClientInstance = new S3Client(clientConfig);
  return s3ClientInstance;
}

export async function generatePresignedUrl(settings: Settings, key: string): Promise<string> {
  const cachedUrl = getCachedPresignedUrl(key);
  if (cachedUrl) return cachedUrl;

  const client = getS3Client(settings);
  const command = new GetObjectCommand({
    Bucket: settings.s3.bucket || '',
    Key: key,
  });

  const expiry = settings.s3.link_expiry || 3600;
  const url = await getSignedUrl(client, command, { expiresIn: expiry });

  cachePresignedUrl(key, url, expiry);
  return url;
}

export async function uploadToS3(settings: Settings, localPath: string, key: string): Promise<void> {
  const client = getS3Client(settings);
  const file = Bun.file(localPath);
  const arrayBuffer = await file.arrayBuffer();

  const command = new PutObjectCommand({
    Bucket: settings.s3.bucket || '',
    Key: key,
    Body: Buffer.from(arrayBuffer),
  });

  await client.send(command);
}

export async function uploadFolderOrFileToS3(settings: Settings, localPath: string, keyPrefix: string = ''): Promise<void> {
  if (!existsSync(localPath)) {
    throw new Error(`Local path does not exist: ${localPath}`);
  }

  const stats = statSync(localPath);
  if (stats.isFile()) {
    const s3Key = keyPrefix ? `${keyPrefix}/${basename(localPath)}` : basename(localPath);
    await uploadToS3(settings, localPath, s3Key);
  } else if (stats.isDirectory()) {
    const files: string[] = [];
    const getFilesRecursively = (dir: string) => {
      const list = readdirSync(dir);
      for (const file of list) {
        const fullPath = join(dir, file);
        const fileStats = statSync(fullPath);
        if (fileStats.isDirectory()) {
          getFilesRecursively(fullPath);
        } else {
          files.push(fullPath);
        }
      }
    };
    getFilesRecursively(localPath);

    const baseParentDir = join(localPath, '..');
    for (const filePath of files) {
      const relPath = relative(baseParentDir, filePath).replace(/\\/g, '/'); // Force forward slashes
      const s3Key = keyPrefix ? `${keyPrefix}/${relPath}` : relPath;
      await uploadToS3(settings, filePath, s3Key);
    }
  }
}
