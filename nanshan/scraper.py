import requests
import json
import time
import os

# Base URL and headers from prd.md
BASE_URL = "https://52kaoyan.top/api/v1"
HEADERS = {
    "Host": "52kaoyan.top",
    "Authorization": "Bearer *****"
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36"
}
CLASS_ID = 25705
REQUEST_DELAY_SECONDS = 0.5 # Delay between requests to be polite to the server

def make_request(url, params):
    """Makes a GET request and handles potential errors."""
    try:
        response = requests.get(url, headers=HEADERS, params=params, timeout=10)
        response.raise_for_status()
        return response.json()
    except requests.exceptions.RequestException as e:
        print(f"Error fetching {url} with params {params}: {e}")
        return None

def get_books():
    """Fetches the list of books for the given classId."""
    print("Fetching books...")
    url = f"{BASE_URL}/tk/famousTk/getBooks"
    params = {"classId": CLASS_ID}
    data = make_request(url, params)
    return data.get("data", []) if data else []

def get_chapters(book_id):
    """Fetches chapters for a given book."""
    print(f"  Fetching chapters for book_id: {book_id}...")
    url = f"{BASE_URL}/tk/famousTk/getChapter"
    params = {"classId": CLASS_ID, "bookId": book_id}
    data = make_request(url, params)
    return data.get("data", {}).get("chapters", []) if data else []

def get_questions(book_id, chapter_id):
    """Fetches questions for a given chapter."""
    print(f"    Fetching questions for chapter_id: {chapter_id}...")
    url = f"{BASE_URL}/tk/famousTk/getQuestion"
    params = {"classId": CLASS_ID, "bookId": book_id, "cid": chapter_id}
    data = make_request(url, params)
    return data.get("data", []) if data else []

def get_comments(question_id):
    """Fetches the first page of comments and extracts 'content' field."""
    print(f"      Fetching comments for question_id: {question_id}...")
    url = f"{BASE_URL}/note/getAll"
    params = {"qid": question_id, "page": 1}
    data = make_request(url, params)
    comments_data = data.get("data", []) if data else []
    return [comment["content"] for comment in comments_data if "content" in comment]

def main():
    """Main function to run the scraper and save the data."""
    start_time = time.time()
    print(f"Starting scraper for classId: {CLASS_ID}")
    
    final_data = []
    books = get_books()
    
    if not books:
        print("No books found or failed to fetch books. Exiting.")
        return

    print(f"Found {len(books)} books.")

    for book in books:
        book_id = book.get("id")
        if not book_id:
            continue
            
        book_data = {
            "book_id": book_id,
            "book_name": book.get("name"),
            "chapters": []
        }
        print(f"\nProcessing book: {book_data['book_name']} (ID: {book_id})")
        
        time.sleep(REQUEST_DELAY_SECONDS)
        chapters = get_chapters(book_id)
        
        for chapter in chapters:
            chapter_id = chapter.get("id")
            if not chapter_id:
                continue

            chapter_data = {
                "chapter_id": chapter_id,
                "chapter_name": chapter.get("name"),
                "questions": []
            }
            
            time.sleep(REQUEST_DELAY_SECONDS)
            questions = get_questions(book_id, chapter_id)
            
            for question in questions:
                question_id = question.get("id")
                if not question_id:
                    continue
                
                time.sleep(REQUEST_DELAY_SECONDS)
                comments = get_comments(question_id)
                question["comments"] = comments
                chapter_data["questions"].append(question)
            
            book_data["chapters"].append(chapter_data)
            
        final_data.append(book_data)

    output_filename = "crawled_data.json"
    with open(output_filename, 'w', encoding='utf-8') as f:
        json.dump(final_data, f, ensure_ascii=False, indent=2)
        
    end_time = time.time()
    print(f"\nScraping finished in {end_time - start_time:.2f} seconds.")
    print(f"Data successfully saved to {os.path.abspath(output_filename)}")

if __name__ == "__main__":
    main()
