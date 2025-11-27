import os
import json
from typing import List, Dict, Optional, Literal
from dataclasses import dataclass, field, asdict
import datetime

# 将数据文件路径固定为“模块所在目录/data/errors.json”，避免因工作目录变化导致路径错误
BASE_DIR = os.path.dirname(os.path.abspath(__file__))
DATA_FILE = os.path.join(BASE_DIR, "data", "errors.json")

@dataclass
class Option:
    label: str
    content: str

@dataclass
class Question:
    id: int
    origin_name: str
    sub_name: str
    content: str
    options: List[Option]
    answer: List[str]
    analysis: str
    comments: List[str]
    type: int = 0
    user_status: Literal["new", "reviewing", "mastered"] = "new"
    last_reviewed: Optional[str] = None
    # 可选：题目来源类别（simulation/real/famous），便于过滤；旧数据可能没有
    source: Optional[str] = None

@dataclass
class ErrorData:
    meta: Dict
    simulation: List[Question] = field(default_factory=list)
    real: List[Question] = field(default_factory=list)
    famous: List[Question] = field(default_factory=list)

def load_data() -> ErrorData:
    """Loads error data from the JSON file."""
    if not os.path.exists(DATA_FILE):
        return ErrorData(meta={"last_sync": None, "version": "1.0"})
    try:
        with open(DATA_FILE, 'r', encoding='utf-8') as f:
            raw_data = json.load(f)
            
            def parse_questions(q_list):
                return [
                    Question(
                        id=q['id'],
                        origin_name=q['origin_name'],
                        sub_name=q['sub_name'],
                        content=q['content'],
                        options=[Option(**o) for o in q['options']],
                        answer=q['answer'],
                        analysis=q['analysis'],
                        comments=q.get('comments', []),
                        type=q.get('type', 0),
                        user_status=q.get('user_status', 'new'),
                        last_reviewed=q.get('last_reviewed'),
                        source=q.get('source')
                    ) for q in q_list
                ]

            return ErrorData(
                meta=raw_data.get('meta', {"last_sync": None, "version": "1.0"}),
                simulation=parse_questions(raw_data.get('simulation', [])),
                real=parse_questions(raw_data.get('real', [])),
                famous=parse_questions(raw_data.get('famous', []))
            )
    except Exception as e:
        print(f"Error loading data from {DATA_FILE}: {e}")
        return ErrorData(meta={"last_sync": None, "version": "1.0"})

def save_data(data: ErrorData):
    """Saves error data to the JSON file."""
    os.makedirs(os.path.dirname(DATA_FILE), exist_ok=True)
    with open(DATA_FILE, 'w', encoding='utf-8') as f:
        json.dump(asdict(data), f, ensure_ascii=False, indent=2)

def update_question_status(question_id: int, new_status: str) -> bool:
    """Updates the status of a specific question."""
    data = load_data()
    timestamp = datetime.datetime.now().isoformat()
    
    found = False
    for category in [data.simulation, data.real, data.famous]:
        for q in category:
            if q.id == question_id:
                q.user_status = new_status
                q.last_reviewed = timestamp
                found = True
                break
        if found:
            break
    
    if found:
        save_data(data)
        return True
    return False
