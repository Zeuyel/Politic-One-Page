# 爬虫需求

bearer token 使用环境变量
python 作为技术栈

## 错题类型

### 模拟卷错题

GET /api/v2/tk/getError?type=5&bookId= HTTP/1.1
Host: 52kaoyan.top

返回：

```
{"code":200,"data":[{"id":17409,"name":"26曲艺3+1","list":[{"id":4958,"cIndex":"","tkExamId":0,"tkTeaId":4958,"simlId":0,"name":"卷一","qids":"147851,147852,147858,147862,147863,147864,147865,147866,147867,147868,147870,147871,147872,147874,147877,147879","step":0,"isComplete":false,"erorrNum":0,"correctNum":0,"isVip":false,"isTry":false},{"id":4959,"cIndex":"","tkExamId":0,"tkTeaId":4959,"simlId":0,"name":"卷二","qids":"147881,147886,147892,147893,147894,147897,147898,147899,147903,147904,147905,147906,147907,147910,147912","step":0,"isComplete":false,"erorrNum":0,"correctNum":0,"isVip":false,"isTry":false}]},{"id":17408,"name":"26余峰六套卷","list":[{"id":4952,"cIndex":"","tkExamId":0,"tkTeaId":4952,"simlId":0,"name":"卷一","qids":"147166,147170,147172,147173,147174,147178,147180,147182,147184,147185,147187,147189,147191,147192,147193,147194","step":0,"isComplete":false,"erorrNum":0,"correctNum":0,"isVip":false,"isTry":false},{"id":4954,"cIndex":"","tkExamId":0,"tkTeaId":4954,"simlId":0,"name":"卷三","qids":"147232,147235,147239,147240,147246,147250,147255,147259,147262","step":0,"isComplete":false,"erorrNum":0,"correctNum":0,"isVip":false,"isTry":false},{"id":4953,"cIndex":"","tkExamId":0,"tkTeaId":4953,"simlId":0,"name":"卷二","qids":"147200,147202,147203,147206,147207,147211,147215,147216,147218,147219,147223,147226,147227,147229,147230","step":0,"isComplete":false,"erorrNum":0,"correctNum":0,"isVip":false,"isTry":false},{"id":4955,"cIndex":"","tkExamId":0,"tkTeaId":4955,"simlId":0,"name":"卷四","qids":"147270,147277,147283,147286,147290,147295","step":0,"isComplete":false,"erorrNum":0,"correctNum":0,"isVip":false,"isTry":false}]},{"id":17401,"name":"26米大李子6","list":[{"id":4920,"cIndex":"","tkExamId":0,"tkTeaId":4920,"simlId":0,"name":"卷一","qids":"146081,146082,146083,146084,146088,146091,146094,146095,146096,146100,146104,146105,146107,146109","step":0,"isComplete":false,"erorrNum":0,"correctNum":0,"isVip":false,"isTry":false}]}],"msg":"操作成功","error":null}
```

参数 "tkExamId","tkTeaId","id", 这里的结构简单，直接就给出了 题目，因此这里的 id 对应题目

### 真题错题

GET /api/v2/tk/getError?type=4&bookId= HTTP/1.1
Host: 52kaoyan.top

```
{"code":200,"data":[{"id":397,"cIndex":"","tkExamId":397,"tkTeaId":0,"simlId":0,"name":"2010年考研真题","qids":"45327,45330,45335,45337,45338,45344,45346,45347,45348,45353,45355","step":0,"isComplete":false,"erorrNum":0,"correctNum":0,"isVip":false,"isTry":false}],"msg":"操作成功","error":null}
```

### 名师题库错题

名师题库结构比较复杂，分为 class ，book，chapter 最后才是 question，参照 ../famousTk/prd.md 中的结构

首先是有错题的 classid 的获取：

GET /api/v2/tk/getError?type=3&bookId= HTTP/1.1
Host: 52kaoyan.top

```
{"code":200,"data":[{"id":25705,"qNum":125,"name":"26大李子米鹏720","username":"26大李子米鹏720","subtitle":"26大李子米鹏720","sort":10,"yeas":"26","isNew":false}],"msg":"操作成功","error":null}
```

id 对应class ，获取book的方式和之前一致。错题获取通过 bookid和classid 直接获取题目：

GET /api/v1/tk/getFamousByError?classId=25705&bookId=1 HTTP/1.1
Host: 52kaoyan.top

```

{"code":200,"data":[{"questions":[{"id":99333,"qId":139904},{"id":99333,"qId":139919},{"id":99333,"qId":139928},{"id":99333,"qId":139930},{"id":99333,"qId":139931},{"id":99333,"qId":139933},{"id":99333,"qId":139934},{"id":99333,"qId":139935},{"id":99333,"qId":139936},{"id":99333,"qId":139938},{"id":99333,"qId":139942},{"id":99333,"qId":139943}],"name":"第二章 世界的物质性及发展规律","classId":25705,"cIndex":99333},{"questions":[{"id":99334,"qId":139952},{"id":99334,"qId":139958},{"id":99334,"qId":139959},{"id":99334,"qId":139960},{"id":99334,"qId":139962},{"id":99334,"qId":139963},{"id":99334,"qId":139966},{"id":99334,"qId":139968},{"id":99334,"qId":139969},{"id":99334,"qId":139972},{"id":99334,"qId":139973},{"id":99334,"qId":139975}],"name":"第三章 实践与认识及其发展规律","classId":25705,"cIndex":99334}],"msg":"操作成功","error":null}
```


这里也就直接获取了 题目的 id 了。


## 题目获取

GET /api/v1/tk/getQuestions?qids=147851%2C147852%2C147858%2C147862%2C147863%2C147864%2C147865%2C147866%2C147867%2C147868%2C147870%2C147871%2C147872%2C147874%2C147877%2C147879&qtype=5&classId=0&tkExamId=0&tkTeaId=4958 HTTP/1.1
Host: 52kaoyan.top

关键参数为 qid 的 构建 其余参数 对应上面。


## 评论获取

参照 ../famousTk/prd.md 中的方式

