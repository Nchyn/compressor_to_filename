self.onmessage=async({data:{file,options}})=>{
 const start=performance.now();let image,canvas;
 const stage=text=>postMessage({type:'stage',text});
 try{
  stage('解码图片');image=await createImageBitmap(file);
  const origWidth=image.width,origHeight=image.height;
  const half=options.preHalf&&file.size>40000000;
  const width=Math.max(1,Math.floor(origWidth*(half?.5:1))),height=Math.max(1,Math.floor(origHeight*(half?.5:1)));
  canvas=new OffscreenCanvas(width,height);const ctx=canvas.getContext('2d');
  if(!ctx)throw Error('无法创建Canvas');ctx.fillStyle=options.background||'#ffffff';ctx.fillRect(0,0,width,height);ctx.imageSmoothingQuality='high';ctx.drawImage(image,0,0,width,height);image.close();image=null;
  const decodeDrawMs=Math.round(performance.now()-start),attempts=[];let chosen=null,low=.45,high=.98,q=.92;
  for(let i=0;i<5;i++){
   stage(`JPEG编码 ${i+1}/5 · 质量参数 ${q.toFixed(3)}`);const t=performance.now();
   const blob=await canvas.convertToBlob({type:'image/jpeg',quality:q});
   if(blob.type!=='image/jpeg')throw Error('浏览器未生成JPEG');
   attempts.push({quality:q,bytes:blob.size,ms:Math.round(performance.now()-t)});
   if(blob.size<20000000){chosen={blob,quality:q};low=q;if(!options.qualityFirst||high-low<.02)break;q=(low+high)/2;}
   else{high=q;if(low===.45&&!chosen){q=Math.max(.45,q-.15);if(attempts.some(a=>a.quality===q))break;}else q=(low+high)/2;}
  }
  if(!chosen)throw Error('5轮预算内未达到20MB；未自动降低分辨率。可主动开启40MB减半后重试。');
  postMessage({type:'result',result:{...chosen,width,height,origWidth,origHeight,half,decodeDrawMs,attempts,ms:Math.round(performance.now()-start)}});
 }catch(e){postMessage({type:'error',message:e.message});}
 finally{image?.close();if(canvas)canvas.width=canvas.height=0;}
};
