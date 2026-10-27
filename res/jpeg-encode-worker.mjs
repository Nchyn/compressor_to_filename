self.onmessage=async({data:{file,options}})=>{
 const start=performance.now();let image,canvas;
 const stage=text=>postMessage({type:'stage',text});
 try{
  stage('解码图片');image=await createImageBitmap(file);
  const origWidth=image.width,origHeight=image.height;
  const target=options.targets?.find(t=>t.width===origWidth&&t.height===origHeight);
  const targetBytes=Math.min(20000000,Math.max(1000,file.size>20000000?20000000:(target?.kb||options.defaultKB||20000)*1000));
  const half=options.preHalf&&file.size>40000000;
  const width=Math.max(1,Math.floor(origWidth*(half?.5:1))),height=Math.max(1,Math.floor(origHeight*(half?.5:1)));
  canvas=new OffscreenCanvas(width,height);const ctx=canvas.getContext('2d');
  if(!ctx)throw Error('无法创建Canvas');ctx.fillStyle=options.background||'#ffffff';ctx.fillRect(0,0,width,height);ctx.imageSmoothingQuality='high';ctx.drawImage(image,0,0,width,height);image.close();image=null;
  const decodeDrawMs=Math.round(performance.now()-start),attempts=[];let chosen=null,low=0,high=1,q=1;
  for(let i=0;i<10;i++){
   stage(`JPEG编码 ${i+1}/10 · 质量参数 ${q.toFixed(3)}`);const t=performance.now();
   const blob=await canvas.convertToBlob({type:'image/jpeg',quality:q});
   if(blob.type!=='image/jpeg')throw Error('浏览器未生成JPEG');
   attempts.push({quality:q,bytes:blob.size,ms:Math.round(performance.now()-t)});
   if(blob.size<targetBytes){if(!chosen||q>chosen.quality)chosen={blob,quality:q};low=q;if(q===1||high-low<.004)break;}
   else high=q;
   q=(low+high)/2;
  }
  if(!chosen)throw Error(`保持分辨率时未达到${targetBytes/1000}KB上限；未自动缩小图片。`);
  postMessage({type:'result',result:{...chosen,targetBytes,width,height,origWidth,origHeight,half,decodeDrawMs,attempts,ms:Math.round(performance.now()-start)}});
 }catch(e){postMessage({type:'error',message:e.message});}
 finally{image?.close();if(canvas)canvas.width=canvas.height=0;}
};
