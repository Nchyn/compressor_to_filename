# Windows prototype acceptance checklist

This prototype is cross-compiled on macOS. A successful PE build is not proof of Windows UI or clipboard behavior.

1. Launch without administrator privileges on Windows 10/11 x64; ensure no missing DLL/runtime prompt.
2. Verify Chinese labels, selectable activity text, Ctrl+C/V/X/A in the URL field, and 100/150/200% display scaling.
3. Save the actual webpage URL and restart; open webpage with the default browser.
4. Allow browser local-network access when prompted. Verify green connected indicator.
5. Paste an image: compressed JPEG should be cached and automatically placed on the file clipboard. Paste into Explorer and the intended messaging application.
6. Drop a test folder (use copies, because the webpage can overwrite originals after confirmation): ZIP should keep the folder name and hierarchy, and be copied automatically. Windows client must not re-encode or repack it.
7. Test “再次复制”; verify the button changes to “已复制” only after clipboard verification.
8. Occupy port 48731 and verify fallback; occupy all five ports and verify an explicit error.
9. Clear cache: confirm current clipboard files are retained and no files outside the cache are touched.
10. Stop/exit: verify service endpoints become unreachable and the process terminates.
11. Test a disallowed Origin and a non-loopback Host; uploads must be rejected.
12. Unsigned prototype may trigger SmartScreen. No signing/notarization or auto-update is provided.
