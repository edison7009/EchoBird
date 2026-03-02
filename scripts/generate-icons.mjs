// 从 SVG 生成三平台图标文件
import sharp from 'sharp';
import pngToIco from 'png-to-ico';
import fs from 'fs';
import path from 'path';

const SVG_PATH = path.resolve('public/ico-desktop.svg');
const BUILD_DIR = path.resolve('build');

async function generate() {
    if (!fs.existsSync(BUILD_DIR)) {
        fs.mkdirSync(BUILD_DIR, { recursive: true });
    }

    const svgBuffer = fs.readFileSync(SVG_PATH);

    // 1. 生成 512x512 PNG（Linux 和 electron-builder 通用）
    const png512 = path.join(BUILD_DIR, 'icon.png');
    await sharp(svgBuffer).resize(512, 512).png().toFile(png512);
    console.log('✅ icon.png (512x512)');

    // 2. 生成 256x256 PNG → 转换为 ICO（Windows）
    const png256Path = path.join(BUILD_DIR, '_icon256.png');
    await sharp(svgBuffer).resize(256, 256).png().toFile(png256Path);
    const icoBuffer = await pngToIco(png256Path);
    fs.writeFileSync(path.join(BUILD_DIR, 'icon.ico'), icoBuffer);
    fs.unlinkSync(png256Path); // 清理临时文件
    console.log('✅ icon.ico (256x256)');

    // 3. 生成 1024x1024 PNG 用于 macOS icns（electron-builder 会自动生成 icns）
    const png1024 = path.join(BUILD_DIR, 'icon.png');
    await sharp(svgBuffer).resize(1024, 1024).png().toFile(png1024);
    console.log('✅ icon.png (1024x1024, for macOS icns auto-generation)');

    // 4. 生成 installerSidebar.bmp（NSIS 安装界面侧边栏 164x314）
    const sidebarWidth = 164;
    const sidebarHeight = 314;
    const iconSize = 120;
    const bg = { r: 15, g: 25, b: 35 }; // #0f1923
    const iconPng = await sharp(svgBuffer).resize(iconSize, iconSize).png().toBuffer();
    const sidebarPng = await sharp({
        create: { width: sidebarWidth, height: sidebarHeight, channels: 3, background: bg }
    }).composite([{
        input: iconPng,
        left: Math.round((sidebarWidth - iconSize) / 2),
        top: Math.round((sidebarHeight - iconSize) / 2)
    }]).removeAlpha().raw().toBuffer();
    // Manually construct 24-bit BMP (bottom-up row order)
    const rowSize = Math.ceil(sidebarWidth * 3 / 4) * 4; // rows padded to 4-byte boundary
    const pixelDataSize = rowSize * sidebarHeight;
    const headerSize = 54;
    const bmpBuf = Buffer.alloc(headerSize + pixelDataSize);
    // BMP file header
    bmpBuf.write('BM', 0);
    bmpBuf.writeUInt32LE(headerSize + pixelDataSize, 2); // file size
    bmpBuf.writeUInt32LE(headerSize, 10); // pixel data offset
    // DIB header (BITMAPINFOHEADER)
    bmpBuf.writeUInt32LE(40, 14); // header size
    bmpBuf.writeInt32LE(sidebarWidth, 18);
    bmpBuf.writeInt32LE(sidebarHeight, 22); // positive = bottom-up
    bmpBuf.writeUInt16LE(1, 26); // color planes
    bmpBuf.writeUInt16LE(24, 28); // bits per pixel
    bmpBuf.writeUInt32LE(pixelDataSize, 34); // image size
    // Write pixels (BMP is bottom-up, BGR order)
    for (let y = 0; y < sidebarHeight; y++) {
        const srcRow = (sidebarHeight - 1 - y) * sidebarWidth * 3;
        const dstRow = headerSize + y * rowSize;
        for (let x = 0; x < sidebarWidth; x++) {
            const srcIdx = srcRow + x * 3;
            const dstIdx = dstRow + x * 3;
            bmpBuf[dstIdx] = sidebarPng[srcIdx + 2];     // B
            bmpBuf[dstIdx + 1] = sidebarPng[srcIdx + 1]; // G
            bmpBuf[dstIdx + 2] = sidebarPng[srcIdx];     // R
        }
    }
    fs.writeFileSync(path.join(BUILD_DIR, 'installerSidebar.bmp'), bmpBuf);
    console.log('✅ installerSidebar.bmp (164x314)');

    // 5. 生成 tray-icon.png（系统托盘图标 256x256，系统会自动缩放，高分辨率保证清晰）
    const trayPath = path.resolve('public/tray-icon.png');
    await sharp(fs.readFileSync(path.resolve('public/ico.svg')))
        .resize(256, 256)
        .png()
        .toFile(trayPath);
    console.log('✅ tray-icon.png (256x256)');

    // 6. 生成 docs/icon.png（网站用，无背景透明版）
    const docsIconPath = path.resolve('docs/icon.png');
    await sharp(fs.readFileSync(path.resolve('public/ico.svg')))
        .resize(512, 512)
        .png()
        .toFile(docsIconPath);
    console.log('✅ docs/icon.png (512x512, transparent)');

    console.log('\n🎉 All icons generated!');
}

generate().catch(e => {
    console.error('❌ Icon generation failed:', e);
    process.exit(1);
});
