# 题目爬虫项目

## 请求头

```
Host: 52kaoyan.top
Authorization: Bearer 481789181504b1b4e6b3c47bee21cb01
```

## 关键节点

 1. GET /api/v1/tk/famousTk/getBooks?classId=25705 HTTP/1.1
 2. GET /api/v1/tk/famousTk/getChapter?classId=25705&bookId=1 HTTP/1.1
 3. GET /api/v1/tk/famousTk/getQuestion?classId=25705&bookId=1&cid=99332 HTTP/1.1
 4. GET /api/v1/note/getAll?qid=139892&page=1 HTTP/1.1
  
1 是获取对应classId 的 Books 的 参数，那么2是更具class和Book获取Chapter以及内部的题目参数也就是cid，3是根据cid、classId、Book获取题目内容。4.是获取题目下面的评论

## 关节节点1


```
HTTP/1.1 200 OK
Server: nginx/1.18.0 (Ubuntu)
Date: Thu, 20 Nov 2025 07:07:45 GMT
Content-Type: application/json; charset=utf-8
Content-Length: 243
Connection: keep-alive
{"code":200,"data":[{"id":1,"name":"马原","sort":1},{"id":316,"name":"毛中特","sort":2},{"id":3609,"name":"新思想","sort":3},{"id":681,"name":"近代史","sort":4},{"id":967,"name":"思修","sort":5}],"msg":"操作成功","error":null}
```

## 关键节点2

```

HTTP/1.1 200 OK
Server: nginx/1.18.0 (Ubuntu)
Date: Thu, 20 Nov 2025 07:09:07 GMT
Content-Type: application/json; charset=utf-8
Connection: keep-alive
Content-Length: 3143
{"code":200,"data":{"totalQ":203,"step":117,"chapters":[{"id":99332,"cIndex":"","tkExamId":0,"tkTeaId":0,"simlId":0,"name":"第一章 导论","qids":"139892,139893,139894,139895,139896,139897,139898,139899,139900,139901,139902,139903","step":12,"isComplete":true,"erorrNum":2,"correctNum":10,"isVip":true,"isTry":false},{"id":99333,"cIndex":"","tkExamId":0,"tkTeaId":0,"simlId":0,"name":"第二章 世界的物质性及发展规律","qids":"139904,139905,139906,139907,139908,139909,139910,139911,139912,139913,139914,139915,139916,139917,139918,139919,139920,139921,139922,139923,139924,139925,139926,139927,139928,139929,139930,139931,139932,139933,139934,139935,139936,139937,139938,139939,139940,139941,139942,139943","step":40,"isComplete":true,"erorrNum":6,"correctNum":34,"isVip":true,"isTry":false},{"id":99334,"cIndex":"","tkExamId":0,"tkTeaId":0,"simlId":0,"name":"第三章 实践与认识及其发展规律","qids":"139944,139945,139946,139947,139948,139949,139950,139951,139952,139953,139954,139955,139956,139957,139958,139959,139960,139961,139962,139963,139964,139965,139966,139967,139968,139969,139970,139971,139972,139973,139974,139975","step":32,"isComplete":true,"erorrNum":12,"correctNum":20,"isVip":true,"isTry":false},{"id":99335,"cIndex":"","tkExamId":0,"tkTeaId":0,"simlId":0,"name":"第四章 人类社会及其发展规律","qids":"139976,139977,139978,139979,139980,139981,139982,139983,139984,139985,139986,139987,139988,139989,139990,139991,139992,139993,139994,139995,139996,139997,139998,139999,140000,140001,140002,140003,140004,140005,140006,140007,140008","step":33,"isComplete":true,"erorrNum":16,"correctNum":17,"isVip":true,"isTry":false},{"id":99336,"cIndex":"","tkExamId":0,"tkTeaId":0,"simlId":0,"name":"第五章 资本主义的本质及规律","qids":"140009,140010,140011,140012,140013,140014,140015,140016,140017,140018,140019,140020,140021,140022,140023,140024,140025,140026,140027,140028,140029,140030,140031,140032,140033,140034,140035,140036,140037,140038,140039,140040,140041,140042,140043,140044,140045,140046,140047,140048","step":0,"isComplete":false,"erorrNum":0,"correctNum":0,"isVip":true,"isTry":false},{"id":99337,"cIndex":"","tkExamId":0,"tkTeaId":0,"simlId":0,"name":"第六章 资本主义的发展及其趋势","qids":"140049,140050,140051,140052,140053,140054,140055,140056,140057,140058,140059,140060,140061,140062,140063,140064,140065,140066,140067,140068,140069,140070,140071","step":0,"isComplete":false,"erorrNum":0,"correctNum":0,"isVip":true,"isTry":false},{"id":99338,"cIndex":"","tkExamId":0,"tkTeaId":0,"simlId":0,"name":"第七章 社会主义的发展及其规律","qids":"140072,140073,140074,140075,140076,140077,140078,140079,140080,140081,140082,140083,140084,140085","step":0,"isComplete":false,"erorrNum":0,"correctNum":0,"isVip":true,"isTry":false},{"id":99339,"cIndex":"","tkExamId":0,"tkTeaId":0,"simlId":0,"name":"第八章 共产主义崇高理想及其最终实现","qids":"140086,140087,140088,140089,140090,140091,140092,140093,140094","step":0,"isComplete":false,"erorrNum":0,"correctNum":0,"isVip":true,"isTry":false}]},"msg":"操作成功","error":null}
```
## 关键节点3

```
HTTP/1.1 200 OK
Server: nginx/1.18.0 (Ubuntu)
Date: Thu, 20 Nov 2025 07:08:37 GMT
Content-Type: application/json; charset=utf-8
Connection: keep-alive
Content-Length: 28602
{"code":200,
"data":[{"id":139892,"tkExamId":0,"teaid":0,"teatkid":0,"classId":25705,"bookId":1,"cId":99332,"pId":0,"title":"面对美国单边主义、保护主义抬头，不断对华出台加征关税等经贸限制措施，习近平主席强调，打关税战没有赢家，同世界作对，将孤立自己。70多年来，中国发展始终靠的是自力更生、艰苦奋斗，从不靠谁的恩赐，更不畏惧任何无理打压。无论外部环境如何变化，中国都将坚定信心、保持定力，集中精力办好自己的事。这里体现的习近平新时代中国特色社会主义思想的世界观和方法论是","a":"必须坚持守正创新","b":"必须坚持自信自立","c":"必须坚持系统观念","d":"必须坚持胸怀天下","correct":"2","simlId":0,"explain":"本题考查习近平新时代中国特色社会主义思想的世界观和方法论。\n习近平新时代中国特色社会主义思想的世界观和方法论（“六个必须坚持”）中，自信自立是中国在历史和实践中形成的精神品格，体现了独立自主的探索和实践精神，以及面对风险挑战时的坚定信念。题干通过“不靠恩赐”“不惧打压”“办好自己的事”，鲜明表达了这一立场。故应选B项。\n守正创新强调在坚持马克思主义基本原理和中国特色社会主义根本方向的基础上推进创新。题干未体现“创新”或“守正”与“创新”的结合，而是聚焦于“自力更生”“不靠谁的恩赐”，故排除A项。系统观念强调整体性、关联性，注重统筹兼顾、系统谋划。题干未涉及多领域协调、全局统筹等内容，故排除C项。胸怀天下强调中国发展与世界发展的互动，倡导人类命运共同体。题干重点是中国自身应对外部挑战的立场，未体现“天下观”或全球治理，故排除D项。\n【刷题笔记】\n党的二十大提出的“六个必须坚持”，是习近平新时代中国特色社会主义思想的世界观、方法论和贯穿其中的立场观点方法的重要体现。\n①坚持人民至上是根本价值立场，体现了历史唯物主义群众史观。\n②坚持自信自立是内在精神特质，体现了客观规律性与主观能动性的有机结合。\n③坚持守正创新是鲜明理论品格，体现了变与不变、继承与发展的内在联系。\n④坚持问题导向是重要实践要求，体现了矛盾的普遍性和客观性。\n⑤坚持系统观念是基本思想和工作方法，体现了辩证唯物主义普遍联系的原理。\n⑥坚持胸怀天下是中国共产党人的境界格局，体现了马克思主义追求人类进步和解放的崇高理想。\n【真题思维点拨】\n做这一类匹配题干信息的题目时，判断哪个选项与题干的信息重合度、衔接度最高即可。材料明确提及了“自力更生”“坚定信心”这些词语，故选B项。","simSource":"","isMSelect":false},...]}
```

## 关键节点4  

```
HTTP/1.1 200 OK
Server: nginx/1.18.0 (Ubuntu)
Date: Thu, 20 Nov 2025 07:22:55 GMT
Content-Type: application/json; charset=utf-8
Connection: keep-alive
Content-Length: 8494

{"code":200,"data":[{"id":349463,"uid":2330673,"qid":0,"classId":0,"tkExamId":0,"simlId":0,"tkTeaId":0,"nickname":"我必选择题一个不错直到政治80➕","avatarFile":"https://52kaoyan.top/api/v1/file/static/img/1747824923368631024tmp_6e4f360c64110f479bc17fbbcf4479ba65edb95af5827ef8.jpg","content":"听mmg的开刷","updateTime":"2025-11-20 00:18:34","voteCount":257,"collectionCount":6,"queList":null},...]}
```

## 目标

抓取classId = 25705 下的所有 题目+评论结构化为 json 文档。
