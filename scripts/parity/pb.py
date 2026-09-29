import sys
def varint(b,i):
    r=s=0
    while True:
        x=b[i];i+=1;r|=(x&0x7f)<<s;s+=7
        if x<0x80:return r,i
def parse(b):
    i=0;out=[]
    while i<len(b):
        k,i=varint(b,i);f,t=k>>3,k&7
        if t==0: v,i=varint(b,i)
        elif t==2:
            l,i=varint(b,i);v=b[i:i+l];i+=l
        elif t==1: v=b[i:i+8];i+=8
        elif t==5: v=b[i:i+4];i+=4
        else: raise ValueError(t)
        out.append((f,t,v))
    return out
