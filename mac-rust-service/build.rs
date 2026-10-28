use std::{env,fs,path::PathBuf};
fn main() {
 let root=PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).parent().unwrap().to_owned();
 let source=root.join("indexv2.html"); println!("cargo:rerun-if-changed={}",source.display());
 let html=fs::read_to_string(source).unwrap(); let mut output=String::new(); let mut remaining=html.as_str();
 while let Some(start)=remaining.find("<script") {
  output.push_str(&remaining[..start]); let tail=&remaining[start..];
  if let Some(end)=tail.find("</script>") {let block=&tail[..end+9];
   if !["googletagmanager.com","hm.baidu.com","gtag("].iter().any(|s|block.contains(s)) {output.push_str(block);}
   remaining=&tail[end+9..];
  } else {remaining=tail;break;}
 }
 output.push_str(remaining);
 output=output.replace("const KV_SIMULATE = false","const KV_SIMULATE = true");
 assert!(!output.contains("googletagmanager.com")&&!output.contains("hm.baidu.com"));
 assert!(output.contains("const KV_SIMULATE = true"));
 fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("compressor.html"),output).unwrap();
}
