import os
import requests
import json
import time
from dotenv import load_dotenv, find_dotenv
from typing import Set

try:
    # 供增量同步读取已有数据文件
    from data_manager import DATA_FILE as DM_DATA_FILE
except Exception:
    DM_DATA_FILE = None

# 可靠加载 .env：向上查找最近的 .env，并允许覆盖已有环境变量
load_dotenv(find_dotenv(), override=True)

BASE_URL = "https://52kaoyan.top/api/v1"
BASE_URL_V2 = "https://52kaoyan.top/api/v2"

HEADERS = {
    "Host": "52kaoyan.top",
    "Authorization": f"Bearer {os.environ.get('TOKEN', '')}",
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36"
}

def refresh_headers():
    """根据当前环境变量 TOKEN 刷新请求头。"""
    global HEADERS
    HEADERS = {
        "Host": "52kaoyan.top",
        "Authorization": f"Bearer {os.environ.get('TOKEN', '')}",
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36"
    }

REQUEST_DELAY_SECONDS = 0.2
# 每批请求的题目数量，避免单次 qids 过长导致失败或过慢
BATCH_SIZE = int(os.environ.get('BATCH_SIZE', '50'))
# 是否获取评论，默认关闭以加快同步。设置 INCLUDE_COMMENTS=1 开启。
INCLUDE_COMMENTS = os.environ.get('INCLUDE_COMMENTS', '0') == '1'
# 是否增量同步，仅抓取本地不存在的新题（默认关闭）
INCREMENTAL = os.environ.get('INCREMENTAL', '0') == '1'

def _parse_sources(val: str):
    """解析 SOURCES 环境变量，返回选择的来源集合。"""
    if not val:
        return {"simulation", "real", "famous"}
    mapping = {
        'simulation': 'simulation', 'sim': 'simulation',
        'real': 'real', 'exam': 'real',
        'famous': 'famous', 'teacher': 'famous'
    }
    out = set()
    for p in val.split(','):
        p = p.strip().lower()
        if p in mapping:
            out.add(mapping[p])
    return out or {"simulation", "real", "famous"}

SELECTED_SOURCES = _parse_sources(os.environ.get('SOURCES', ''))

def set_runtime_options(include_comments=None, batch_size=None, token=None):
    """在运行期更新抓取参数与 TOKEN。供交互式界面调用。"""
    global INCLUDE_COMMENTS, BATCH_SIZE
    if include_comments is not None:
        INCLUDE_COMMENTS = bool(int(include_comments)) if isinstance(include_comments, (int, str)) else bool(include_comments)
        os.environ['INCLUDE_COMMENTS'] = '1' if INCLUDE_COMMENTS else '0'
    if batch_size is not None:
        try:
            BATCH_SIZE = int(batch_size)
            os.environ['BATCH_SIZE'] = str(BATCH_SIZE)
        except Exception:
            pass
    if token is not None:
        os.environ['TOKEN'] = token
        refresh_headers()

def get_runtime_options():
    return {
        'include_comments': INCLUDE_COMMENTS,
        'batch_size': BATCH_SIZE,
        'token_set': bool(os.environ.get('TOKEN')),
        'incremental': INCREMENTAL,
        'sources': sorted(list(SELECTED_SOURCES)),
    }

def make_request(url, params=None, version='v1'):
    """Makes a GET request and handles potential errors, with versioning."""
    full_url = f"{BASE_URL_V2 if version == 'v2' else BASE_URL}{url}"
    print(f"Fetching from: {full_url} with params: {params}")
    try:
        response = requests.get(full_url, headers=HEADERS, params=params, timeout=15)
        response.raise_for_status()
        return response.json()
    except requests.exceptions.RequestException as e:
        print(f"Error fetching {full_url} with params {params}: {e}")
        return None

def get_question_details(qids):
    """获取一批题目详情（不分批）。"""
    if not qids:
        return []
    qids_str = ",".join(map(str, qids))
    url = "/tk/getQuestions"
    params = {"qids": qids_str}
    data = make_request(url, params)
    return (data.get("data") or []) if data else []

def get_question_details_batched(qids, batch_size=None):
    """按批次获取题目详情，提升稳定性。支持运行期调整批大小。"""
    if batch_size is None:
        try:
            batch_size = int(os.environ.get('BATCH_SIZE', str(BATCH_SIZE)))
        except Exception:
            batch_size = BATCH_SIZE
    all_details = []
    for i in range(0, len(qids), batch_size):
        batch = qids[i:i+batch_size]
        details = get_question_details(batch)
        if details:
            all_details.extend(details)
        time.sleep(REQUEST_DELAY_SECONDS)
    return all_details

def _load_existing_ids() -> dict:
    """读取本地已存在的题目 ID 集合，用于增量同步。"""
    ids = {"simulation": set(), "real": set(), "famous": set()}
    try:
        if DM_DATA_FILE and os.path.exists(DM_DATA_FILE):
            with open(DM_DATA_FILE, 'r', encoding='utf-8') as f:
                jd = json.load(f)
            for k in ids.keys():
                for q in (jd.get(k) or []):
                    qid = q.get('id')
                    if isinstance(qid, int):
                        ids[k].add(qid)
    except Exception:
        pass
    return ids

def get_comments(question_id):
    """Fetches comments for a given question ID."""
    url = "/note/getAll"
    params = {"qid": question_id, "page": 1}
    data = make_request(url, params)
    comments_data = (data.get("data") or []) if data else []
    return [comment["content"] for comment in comments_data if "content" in comment]

def _normalize_question(question_data, source_type, origin_name, sub_name=""):
    """Normalizes a single question's data into the defined structure."""
    if not question_data:
        return None
    
    normalized = {
        "id": question_data.get("id"),
        "origin_name": origin_name,
        "sub_name": sub_name,
        "type": question_data.get("type", 0),
        "content": question_data.get("title", ""),
        "options": [],
        "answer": [],
        "analysis": question_data.get("explain", ""),
        "user_status": "new",
        "last_reviewed": None,
        "comments": [],
        "source": source_type
    }

    for key in ['a', 'b', 'c', 'd']:
        if question_data.get(key):
            normalized["options"].append({"label": key.upper(), "content": question_data[key]})
    
    correct_raw = str(question_data.get("correct", "")).strip()
    if correct_raw:
        if correct_raw.isdigit():
            labels = ['A', 'B', 'C', 'D']
            for digit in correct_raw:
                idx = int(digit) - 1
                if 0 <= idx < len(labels):
                    normalized["answer"].append(labels[idx])
        else:
            normalized["answer"] = [label.strip().upper() for label in correct_raw.split(',')]

    return normalized

def fetch_simulation_errors(existing: Set[int] | None = None):
    print("Fetching Simulation Errors (Type 5)...")
    all_questions = []
    
    papers_data = make_request("/tk/getError", {"type": 5}, version='v2')
    
    if not papers_data:
        return []

    for paper in (papers_data.get("data") or []):
        origin_name = paper.get("name", "Unknown Paper")
        for chapter_or_roll in (paper.get("list") or []):
            sub_name = chapter_or_roll.get("name", "Unknown Roll")
            qids_str = chapter_or_roll.get("qids")
            if not qids_str:
                continue
            
            qids = list(map(int, qids_str.split(',')))
            if existing:
                qids = [q for q in qids if q not in existing]
                if not qids:
                    continue
            details = get_question_details_batched(qids)
            
            for q_data in details:
                time.sleep(REQUEST_DELAY_SECONDS)
                comments = get_comments(q_data.get("id")) if INCLUDE_COMMENTS else []
                normalized_q = _normalize_question(q_data, "simulation", origin_name, sub_name)
                if normalized_q:
                    normalized_q["comments"] = comments
                    all_questions.append(normalized_q)
    return all_questions

def fetch_real_exam_errors(existing: Set[int] | None = None):
    print("Fetching Real Exam Errors (Type 4)...")
    all_questions = []
    
    papers_data = make_request("/tk/getError", {"type": 4}, version='v2')

    if not papers_data:
        return []

    for paper in (papers_data.get("data") or []):
        origin_name = paper.get("name", "Unknown Real Exam")
        sub_name = ""
        qids_str = paper.get("qids")
        if not qids_str:
            continue
            
        qids = list(map(int, qids_str.split(',')))
        if existing:
            qids = [q for q in qids if q not in existing]
            if not qids:
                continue
        details = get_question_details_batched(qids)
        
        for q_data in details:
            time.sleep(REQUEST_DELAY_SECONDS)
            comments = get_comments(q_data.get("id")) if INCLUDE_COMMENTS else []
            normalized_q = _normalize_question(q_data, "real", origin_name, sub_name)
            if normalized_q:
                normalized_q["comments"] = comments
                all_questions.append(normalized_q)
    return all_questions

def fetch_famous_errors(existing: Set[int] | None = None):
    print("Fetching Famous Teacher Errors (Type 3)...")
    all_questions = []

    classes_data = make_request("/tk/getError", {"type": 3}, version='v2')
    if not classes_data:
        return []

    for class_item in (classes_data.get("data") or []):
        class_id = class_item.get("id")
        origin_name = class_item.get("name", "Unknown Famous Teacher Class")
        if not class_id:
            continue

        # 注意：这里传相对路径，避免与 BASE_URL 拼接成重复前缀
        url_books = "/tk/famousTk/getBooks"
        params_books = {"classId": class_id}
        books_data = make_request(url_books, params_books)
        books = (books_data.get("data") or []) if books_data else []
        
        if not books:
            books = [{"id": 1, "name": "Default Book"}]
            
        for book in books:
            book_id = book.get("id")
            if not book_id:
                continue

            questions_list_data = make_request("/tk/getFamousByError", {"classId": class_id, "bookId": book_id})
            if not questions_list_data:
                continue

            for chapter_data in (questions_list_data.get("data") or []):
                sub_name = chapter_data.get("name", "Unknown Chapter")
                qids = [q_item.get("qId") for q_item in (chapter_data.get("questions") or []) if q_item.get("qId")]
                
                if not qids:
                    continue
                
                if existing:
                    qids = [q for q in qids if q not in existing]
                    if not qids:
                        continue
                details = get_question_details_batched(qids)
                
                for q_data in details:
                    time.sleep(REQUEST_DELAY_SECONDS)
                    comments = get_comments(q_data.get("id")) if INCLUDE_COMMENTS else []
                    normalized_q = _normalize_question(q_data, "famous", origin_name, sub_name)
                    if normalized_q:
                        normalized_q["comments"] = comments
                        all_questions.append(normalized_q)
    return all_questions

def fetch_all_errors():
    all_categorized_errors = {
        "meta": {
            "last_sync": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "version": "1.0"
        },
        "simulation": [],
        "real": [],
        "famous": []
    }
    print(f"Selected sources: {sorted(list(SELECTED_SOURCES))} | INCLUDE_COMMENTS={INCLUDE_COMMENTS} | INCREMENTAL={INCREMENTAL}")

    existing = _load_existing_ids() if INCREMENTAL else {"simulation": set(), "real": set(), "famous": set()}

    # 逐类抓取，并打印数量，便于调试
    if 'simulation' in SELECTED_SOURCES:
        simulation_errors = fetch_simulation_errors(existing.get('simulation'))
        all_categorized_errors["simulation"].extend(simulation_errors)
        print(f"Collected simulation: {len(simulation_errors)}")
        time.sleep(REQUEST_DELAY_SECONDS)

    if 'real' in SELECTED_SOURCES:
        real_errors = fetch_real_exam_errors(existing.get('real'))
        all_categorized_errors["real"].extend(real_errors)
        print(f"Collected real: {len(real_errors)}")
        time.sleep(REQUEST_DELAY_SECONDS)

    if 'famous' in SELECTED_SOURCES:
        famous_errors = fetch_famous_errors(existing.get('famous'))
        all_categorized_errors["famous"].extend(famous_errors)
        print(f"Collected famous: {len(famous_errors)}")

    return all_categorized_errors
