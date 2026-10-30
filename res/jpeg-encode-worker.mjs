self.onmessage=async({data:{file,options}})=>{
 const start=performance.now();let image,canvas;
 const stage=text=>postMessage({type:'stage',text});
 try{
  stage('解码图片');image=await createImageBitmap(file);
  const origWidth=image.width,origHeight=image.height;
   if(file.size>20000000){
    let width=origWidth,height=origHeight,half=false;const targetBytes=20000000,attempts=[];
    postMessage({type:'target',targetBytes,width,height});
    async function encode(){
     canvas=new OffscreenCanvas(width,height);const ctx=canvas.getContext('2d');
     if(!ctx)throw Error('无法创建Canvas');ctx.fillStyle=options.background||'#ffffff';ctx.fillRect(0,0,width,height);ctx.imageSmoothingQuality='high';ctx.drawImage(image,0,0,width,height);
     stage(half?'仍达到20MB · 原图宽高减半 · Q0.900':'大于20MB · 原尺寸编码 · Q0.900');const t=performance.now();
     const blob=await canvas.convertToBlob({type:'image/jpeg',quality:0.9});if(blob.type!=='image/jpeg')throw Error('浏览器未生成JPEG');
     attempts.push({quality:0.9,bytes:blob.size,width,height,ms:Math.round(performance.now()-t)});return blob;
    }
    let blob=await encode();
    if(blob.size>=targetBytes){half=true;width=Math.max(1,Math.floor(origWidth/2));height=Math.max(1,Math.floor(origHeight/2));canvas.width=canvas.height=0;blob=await encode();}
    image.close();image=null;
    if(blob.size>=targetBytes)throw Error('原图宽高减半并以Q0.9编码后仍达到20MB，无法复制至客户端；未继续缩小。');
    postMessage({type:'result',result:{blob,quality:0.9,targetBytes,width,height,origWidth,origHeight,half,decodeDrawMs:0,attempts,ms:Math.round(performance.now()-start)}});return;
   }

   const target=options.targets?.find(t=>t.width===origWidth&&t.height===origHeight);
  const fixedQuality=file.size>10000000;
  const requestedKB=options.manualMode?options.manualKB:(target?.kb||options.defaultKB||20000);
  const targetBytes=fixedQuality?20000000:Math.min(20000000,Math.max(1000,(Number(requestedKB)||300)*1000));
  postMessage({type:'target',targetBytes,width:origWidth,height:origHeight});
   const half=options.preHalf&&file.size>40000000;
  const width=Math.max(1,Math.floor(origWidth*(half?.5:1))),height=Math.max(1,Math.floor(origHeight*(half?.5:1)));
  canvas=new OffscreenCanvas(width,height);const ctx=canvas.getContext('2d');
  if(!ctx)throw Error('无法创建Canvas');ctx.fillStyle=options.background||'#ffffff';ctx.fillRect(0,0,width,height);ctx.imageSmoothingQuality='high';ctx.drawImage(image,0,0,width,height);image.close();image=null;
  const decodeDrawMs=Math.round(performance.now()-start),attempts=[],maxAttempts=4;let chosen=null,low=.5,high=.99,q=fixedQuality?.98:.99;
  for(let i=0;i<maxAttempts;i++){
   stage(`JPEG编码 ${i+1}/${maxAttempts} · 质量参数 ${q.toFixed(3)}`);const t=performance.now();
   const blob=await canvas.convertToBlob({type:'image/jpeg',quality:q});
   if(blob.type!=='image/jpeg')throw Error('浏览器未生成JPEG');
   attempts.push({quality:q,bytes:blob.size,ms:Math.round(performance.now()-t)});
   if(blob.size<targetBytes){if(!chosen||q>chosen.quality)chosen={blob,quality:q};low=q;if(i===0)break;}
   else {high=q;if(q===0.5)break;}
   q=(i===0&&!chosen)?0.5:(low+high)/2;
  }
  if(!chosen)throw Error(`4次编码内未达到${targetBytes/1000}KB上限；已降低质量，未自动缩小图片。`);
  postMessage({type:'result',result:{...chosen,targetBytes,width,height,origWidth,origHeight,half,decodeDrawMs,attempts,ms:Math.round(performance.now()-start)}});
 }catch(e){postMessage({type:'error',message:e.message});}
 finally{image?.close();if(canvas)canvas.width=canvas.height=0;}
};
