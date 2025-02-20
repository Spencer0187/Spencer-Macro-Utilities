import React from 'react';
import styled, { createGlobalStyle } from 'styled-components';

// ===== Global Styles =====
const GlobalStyle = createGlobalStyle`
  :root {
    --bg-top: #121212;
    --bg-bottom: #0a0a0a;
    --text: #e0e0e0;
  }

  html {
    scroll-behavior: smooth;
  }

  * {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }

  body {
    /* FORCED INLINE GRADIENT FOR TESTING */
    background: linear-gradient(145deg, var(--bg-bottom) 0%, var(--bg-top) 100%) !important;
    color: var(--text);
    line-height: 1.6;
    font-family: 'Inter', system-ui, sans-serif;
    padding: 2rem 1rem;
    min-height: 100vh;
  }
`;

// ===== Styled Components =====
const Container = styled.div`
  max-width: 1200px;
  margin: 0 auto;
`;

const FeatureShowcase = styled.section`
  display: grid;
  gap: 2rem;
  margin: 4rem 0 2rem;
  @media (min-width: 768px) {
    grid-template-columns: 1fr 1fr;
    align-items: start;
  }
`;

const Screenshot = styled.div`
  border-radius: 12px;
  overflow: hidden;
  border: 1px solid #333;

  img {
    width: 100%;
    height: auto;
    display: block;
    /* FORCED LARGER IMAGE SIZE FOR TESTING */
    max-height: 700px !important;
    object-fit: contain;
  }
`;


// ===== Updated Main Component =====
const App = () => {
  const screenshotUrl1 = 'https://github.com/user-attachments/assets/f38363ad-da99-4c00-92a5-39faf1dd8c8c';

  return (
    <>
      <GlobalStyle />
      <Container>
        <FeatureShowcase>
          <Screenshot>
            <img src={screenshotUrl1} alt="Test Screenshot" style={{ maxHeight: '700px !important' }} /> {/* Inline style as extra measure */}
          </Screenshot>
        </FeatureShowcase>
      </Container>
    </>
  );
};

export default App;
